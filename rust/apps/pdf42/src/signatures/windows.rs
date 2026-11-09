use super::*;
use std::{
    io::Read,
    mem::size_of,
    path::PathBuf,
    ptr::{null, null_mut},
    sync::{
        mpsc::{self, Receiver},
        Arc,
    },
};
use windows_sys::Win32::{Foundation::GetLastError, Security::Cryptography::*};

const ENCODING: u32 = X509_ASN_ENCODING | PKCS_7_ASN_ENCODING;
const MAX_SIGNERS: i32 = 8;
const MAX_VERIFY_BYTES: usize = 512 * 1024 * 1024;

pub struct Worker(Receiver<Result<win42::ReportRequest, String>>);

impl Worker {
    pub fn start(path: PathBuf) -> Self {
        Self::spawn(move || inspect(path))
    }

    pub fn start_view(source: Arc<[u8]>, name: String) -> Self {
        Self::spawn(move || inspect_bytes(&source, &name))
    }

    fn spawn(task: impl FnOnce() -> Result<win42::ReportRequest, String> + Send + 'static) -> Self {
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = send.send(task());
        });
        Self(receive)
    }

    pub fn poll(&self) -> Option<Result<win42::ReportRequest, String>> {
        match self.0.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err("Signature verification worker stopped".into()))
            }
        }
    }
}

fn inspect(path: PathBuf) -> Result<win42::ReportRequest, String> {
    let mut data = Vec::new();
    std::fs::File::open(&path)
        .and_then(|file| file.take(MAX_FILE as u64 + 1).read_to_end(&mut data))
        .map_err(|error| error.to_string())?;
    let name = path
        .file_name()
        .map_or_else(|| "PDF".into(), |name| name.to_string_lossy());
    inspect_bytes(&data, &name)
}

fn inspect_bytes(data: &[u8], name: &str) -> Result<win42::ReportRequest, String> {
    let doc = load(data)?;
    let mut rows = Vec::new();
    let mut work = 0usize;
    for field in fields(&doc)? {
        let signature = match envelope(&field, data) {
            Ok(signature) => signature,
            Err(error) => {
                rows.push(vec![field.name, String::new(), error, String::new()]);
                continue;
            }
        };
        let signed_end = signature.right.end;
        work = work.saturating_add(signed_end);
        if work > MAX_VERIFY_BYTES {
            return Err("PDF signature verification work limit exceeded".into());
        }
        if !is_bound_to_revision(data, &signature)? {
            rows.push(vec![
                field.name,
                String::new(),
                "Signature field is absent from its signed revision".into(),
                String::new(),
            ]);
            continue;
        }
        let whole = signed_end == data.len();
        let coverage = if whole {
            "Entire file"
        } else {
            "Earlier revision. Unsigned bytes follow"
        };
        let buffers = [
            &data[signature.left.clone()],
            &data[signature.right.clone()],
        ];
        let count = unsafe {
            CryptGetMessageSignerCount(ENCODING, signature.cms.as_ptr(), signature.cms.len() as u32)
        };
        if !(1..=MAX_SIGNERS).contains(&count) {
            rows.push(vec![
                field.name,
                String::new(),
                "Unsupported or damaged CMS signer list".into(),
                coverage.into(),
            ]);
            continue;
        }
        work = work.saturating_add(signed_end.saturating_mul(count as usize - 1));
        if work > MAX_VERIFY_BYTES {
            return Err("PDF signature verification work limit exceeded".into());
        }
        for index in 0..count as u32 {
            let (signer, status) = verify(&signature, buffers, index);
            rows.push(vec![field.name.clone(), signer, status, coverage.into()]);
        }
    }
    let summary = if rows.is_empty() {
        format!("{name} · No signed form fields")
    } else {
        format!("{name} · Windows cached certificate-chain check at current system time")
    };
    Ok(win42::ReportRequest {
        title: "PDF Signature Verification".into(),
        summary,
        columns: vec!["Field", "Signer", "Verification", "Coverage"],
        column_colors: vec![None; 4],
        rows,
        highlight_controls: false,
        compact_first_column: false,
        allow_keep: false,
        keeping: false,
    })
}

struct Certificate(*mut CERT_CONTEXT);
impl Drop for Certificate {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CertFreeCertificateContext(self.0) };
        }
    }
}

struct Chain(*mut CERT_CHAIN_CONTEXT);
impl Drop for Chain {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CertFreeCertificateChain(self.0) };
        }
    }
}

fn verify(signature: &Signature<'_>, bytes: [&[u8]; 2], index: u32) -> (String, String) {
    let parameters = CRYPT_VERIFY_MESSAGE_PARA {
        cbSize: size_of::<CRYPT_VERIFY_MESSAGE_PARA>() as u32,
        dwMsgAndCertEncodingType: ENCODING,
        ..Default::default()
    };
    let pointers = bytes.map(|bytes| bytes.as_ptr());
    let lengths = bytes.map(|bytes| bytes.len() as u32);
    let mut cert = Certificate(null_mut());
    unsafe {
        if CryptVerifyDetachedMessageSignature(
            &parameters,
            index,
            signature.cms.as_ptr(),
            signature.cms.len() as u32,
            pointers.len() as u32,
            pointers.as_ptr(),
            lengths.as_ptr(),
            &mut cert.0,
        ) == 0
            || cert.0.is_null()
        {
            return (
                String::new(),
                format!("Signature verification failed (0x{:08X})", GetLastError()),
            );
        }
    }
    let signer = subject(&cert);
    let status = match trust(&cert) {
        Ok(()) => "Signature intact. Certificate chain verified".into(),
        Err(code) => format!("Signature intact. Certificate trust not established (0x{code:08X})"),
    };
    (signer, status)
}

fn subject(cert: &Certificate) -> String {
    let count = unsafe {
        CertGetNameStringW(
            cert.0,
            CERT_NAME_SIMPLE_DISPLAY_TYPE,
            0,
            null(),
            null_mut(),
            0,
        )
    };
    if !(2..=2048).contains(&count) {
        return "Unknown signer".into();
    }
    let mut name = vec![0; count as usize];
    let written = unsafe {
        CertGetNameStringW(
            cert.0,
            CERT_NAME_SIMPLE_DISPLAY_TYPE,
            0,
            null(),
            name.as_mut_ptr(),
            count,
        )
    };
    if written != count {
        return "Unknown signer".into();
    }
    String::from_utf16_lossy(&name[..name.len() - 1])
        .chars()
        .filter(|c| !c.is_control())
        .take(150)
        .collect()
}

fn trust(cert: &Certificate) -> Result<(), u32> {
    let parameters = CERT_CHAIN_PARA {
        cbSize: size_of::<CERT_CHAIN_PARA>() as u32,
        ..Default::default()
    };
    let flags = CERT_CHAIN_CACHE_ONLY_URL_RETRIEVAL
        | CERT_CHAIN_DISABLE_AUTH_ROOT_AUTO_UPDATE
        | CERT_CHAIN_REVOCATION_CHECK_CACHE_ONLY
        | CERT_CHAIN_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT;
    let mut chain = Chain(null_mut());
    unsafe {
        if CertGetCertificateChain(
            null_mut(),
            cert.0,
            null(),
            (*cert.0).hCertStore,
            &parameters,
            flags,
            null(),
            &mut chain.0,
        ) == 0
            || chain.0.is_null()
        {
            return Err(GetLastError());
        }
    }
    let policy = CERT_CHAIN_POLICY_PARA {
        cbSize: size_of::<CERT_CHAIN_POLICY_PARA>() as u32,
        ..Default::default()
    };
    let mut status = CERT_CHAIN_POLICY_STATUS {
        cbSize: size_of::<CERT_CHAIN_POLICY_STATUS>() as u32,
        ..Default::default()
    };
    if unsafe {
        CertVerifyCertificateChainPolicy(CERT_CHAIN_POLICY_BASE, chain.0, &policy, &mut status)
    } == 0
    {
        return Err(unsafe { GetLastError() });
    }
    if status.dwError == 0 {
        Ok(())
    } else {
        Err(status.dwError)
    }
}
