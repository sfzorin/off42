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
use windows_sys::Win32::{
    Foundation::{GetLastError, SetLastError, ERROR_CANCELLED, HWND},
    Security::Cryptography::{UI::CryptUIDlgSelectCertificateFromStore, *},
    System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED},
};

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

struct CertificateStore(HCERTSTORE);
impl Drop for CertificateStore {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CertCloseStore(self.0, 0) };
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
    let cert = match verify_cms(signature.cms, bytes, index) {
        Ok(cert) => cert,
        Err(code) => {
            return (
                String::new(),
                format!("Signature verification failed (0x{code:08X})"),
            )
        }
    };
    let signer = subject(&cert);
    let status = match trust(&cert) {
        Ok(()) => "Signature intact. Certificate chain verified".into(),
        Err(code) => format!("Signature intact. Certificate trust not established (0x{code:08X})"),
    };
    (signer, status)
}

fn verify_cms(cms: &[u8], bytes: [&[u8]; 2], index: u32) -> Result<Certificate, u32> {
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
            cms.as_ptr(),
            cms.len() as u32,
            pointers.len() as u32,
            pointers.as_ptr(),
            lengths.as_ptr(),
            &mut cert.0,
        ) == 0
            || cert.0.is_null()
        {
            return Err(GetLastError());
        }
    }
    Ok(cert)
}

pub struct SignWorker(Receiver<Result<Option<PathBuf>, String>>);

impl SignWorker {
    pub fn start(input: pdf42_core::SignatureInput, path: PathBuf, parent: usize) -> Self {
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let result = sign_copy(input, path, parent);
            let _ = sender.send(result);
        });
        Self(receiver)
    }

    pub fn poll(&self) -> Option<Result<Option<PathBuf>, String>> {
        match self.0.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err("PDF signing worker stopped".into())),
        }
    }
}

fn sign_copy(
    input: pdf42_core::SignatureInput,
    path: PathBuf,
    parent: usize,
) -> Result<Option<PathBuf>, String> {
    let apartment = unsafe { CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) };
    if apartment < 0 {
        return Err(format!(
            "Cannot open the certificate picker (0x{:08X})",
            apartment as u32
        ));
    }
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }
    let _apartment = Apartment;
    let Some(cert) = choose_certificate(parent as HWND)? else {
        return Ok(None);
    };
    let now = chrono::Utc::now();
    let draft = input.prepare(
        &subject(&cert),
        &now.format("D:%Y%m%d%H%M%SZ").to_string(),
        &now.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
    )?;
    let bytes = draft.signed_bytes();
    let cms = sign_cms(&cert, bytes)?;
    if cms_der(&cms).is_none() {
        return Err("The selected certificate provider did not return detached CMS".into());
    }
    verify_cms(&cms, bytes, 0)
        .map_err(|code| format!("Created signature failed verification (0x{code:08X})"))?;
    let signed = draft.finish(&cms)?;
    suite42_core::write_new(&path, &signed).map_err(|error| error.to_string())?;
    Ok(Some(path))
}

fn choose_certificate(owner: HWND) -> Result<Option<Certificate>, String> {
    use windows_sys::core::w;
    let source = CertificateStore(unsafe {
        CertOpenStore(
            CERT_STORE_PROV_SYSTEM_W,
            0,
            0,
            CERT_SYSTEM_STORE_CURRENT_USER
                | CERT_STORE_OPEN_EXISTING_FLAG
                | CERT_STORE_READONLY_FLAG,
            w!("MY").cast(),
        )
    });
    if source.0.is_null() {
        return Err(format!(
            "Cannot read the Windows personal certificate store (0x{:08X})",
            unsafe { GetLastError() }
        ));
    }
    let choices =
        CertificateStore(unsafe { CertOpenStore(CERT_STORE_PROV_MEMORY, 0, 0, 0, null()) });
    if choices.0.is_null() {
        return Err("Cannot prepare the certificate picker".into());
    }
    let mut cursor = null_mut();
    let mut count = 0;
    loop {
        cursor = unsafe { CertEnumCertificatesInStore(source.0, cursor) };
        if cursor.is_null() {
            break;
        }
        if unsafe { eligible(cursor) } {
            if unsafe {
                CertAddCertificateContextToStore(
                    choices.0,
                    cursor,
                    CERT_STORE_ADD_ALWAYS,
                    null_mut(),
                )
            } == 0
            {
                unsafe { CertFreeCertificateContext(cursor) };
                return Err("Cannot display the available certificates".into());
            }
            count += 1;
        }
    }
    if count == 0 {
        return Err("No current personal certificate with a signing key was found".into());
    }
    unsafe {
        SetLastError(0);
        let selected = CryptUIDlgSelectCertificateFromStore(
            choices.0,
            owner,
            w!("Choose a PDF signing certificate"),
            w!("Select a certificate you are authorized to use for this PDF copy."),
            0,
            0,
            null(),
        );
        if selected.is_null() {
            let code = GetLastError();
            if matches!(code, 0 | ERROR_CANCELLED) {
                return Ok(None);
            }
            return Err(format!("Certificate selection ended (0x{code:08X})"));
        }
        let choice = Certificate(selected);
        let original = CertFindCertificateInStore(
            source.0,
            X509_ASN_ENCODING,
            0,
            CERT_FIND_EXISTING,
            choice.0.cast(),
            null(),
        );
        if original.is_null() {
            return Err(
                "The selected certificate is no longer available in the personal store".into(),
            );
        }
        Ok(Some(Certificate(original)))
    }
}

unsafe fn eligible(cert: *const CERT_CONTEXT) -> bool {
    let info = unsafe { (*cert).pCertInfo };
    if info.is_null() || unsafe { CertVerifyTimeValidity(null(), info) } != 0 {
        return false;
    }
    let has_key = [
        CERT_KEY_PROV_INFO_PROP_ID,
        CERT_KEY_CONTEXT_PROP_ID,
        CERT_NCRYPT_KEY_HANDLE_PROP_ID,
    ]
    .into_iter()
    .any(|property| {
        let mut size = 0;
        unsafe { CertGetCertificateContextProperty(cert, property, null_mut(), &mut size) != 0 }
    });
    if !has_key {
        return false;
    }
    let usage_extension =
        unsafe { CertFindExtension(szOID_KEY_USAGE, (*info).cExtension, (*info).rgExtension) };
    if usage_extension.is_null() {
        return true;
    }
    let mut usage = [0u8; 2];
    unsafe {
        CertGetIntendedKeyUsage(
            X509_ASN_ENCODING,
            info,
            usage.as_mut_ptr(),
            usage.len() as u32,
        ) != 0
            && u32::from(usage[0])
                & (CERT_DIGITAL_SIGNATURE_KEY_USAGE | CERT_NON_REPUDIATION_KEY_USAGE)
                != 0
    }
}

fn sign_cms(cert: &Certificate, bytes: [&[u8]; 2]) -> Result<Vec<u8>, String> {
    let mut certs = [cert.0];
    let parameters = CRYPT_SIGN_MESSAGE_PARA {
        cbSize: size_of::<CRYPT_SIGN_MESSAGE_PARA>() as u32,
        dwMsgEncodingType: ENCODING,
        pSigningCert: cert.0,
        HashAlgorithm: CRYPT_ALGORITHM_IDENTIFIER {
            pszObjId: szOID_NIST_sha256.cast_mut(),
            ..Default::default()
        },
        cMsgCert: certs.len() as u32,
        rgpMsgCert: certs.as_mut_ptr(),
        ..Default::default()
    };
    let pointers = bytes.map(|bytes| bytes.as_ptr());
    let lengths = bytes.map(|bytes| bytes.len() as u32);
    let mut cms = vec![0; 64 * 1024];
    let mut length = cms.len() as u32;
    if unsafe {
        CryptSignMessage(
            &parameters,
            1,
            pointers.len() as u32,
            pointers.as_ptr(),
            lengths.as_ptr(),
            cms.as_mut_ptr(),
            &mut length,
        )
    } == 0
    {
        return Err(format!("Certificate signing failed (0x{:08X})", unsafe {
            GetLastError()
        }));
    }
    if length == 0 || length as usize > cms.len() {
        return Err("Certificate provider returned an invalid signature length".into());
    }
    cms.truncate(length as usize);
    Ok(cms)
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

#[cfg(test)]
mod tests {
    use super::*;

    struct Provider {
        handle: usize,
        name: Vec<u16>,
    }

    impl Provider {
        fn create() -> Self {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let name = format!("off42-signing-audit-{}-{nonce}", std::process::id())
                .encode_utf16()
                .chain([0])
                .collect::<Vec<_>>();
            let mut handle = 0;
            assert_ne!(
                unsafe {
                    CryptAcquireContextW(
                        &mut handle,
                        name.as_ptr(),
                        MS_ENH_RSA_AES_PROV_W,
                        PROV_RSA_AES,
                        CRYPT_NEWKEYSET,
                    )
                },
                0,
                "creating disposable RSA key container: 0x{:08X}",
                unsafe { GetLastError() }
            );
            Self { handle, name }
        }
    }

    impl Drop for Provider {
        fn drop(&mut self) {
            unsafe {
                CryptReleaseContext(self.handle, 0);
                let mut unused = 0;
                CryptAcquireContextW(
                    &mut unused,
                    self.name.as_ptr(),
                    MS_ENH_RSA_AES_PROV_W,
                    PROV_RSA_AES,
                    CRYPT_DELETEKEYSET,
                );
            }
        }
    }

    #[test]
    fn windows_signs_prepared_pdf_and_detects_signed_byte_changes() {
        use lopdf::dictionary;
        use windows_sys::core::w;

        let provider = Provider::create();
        let mut key = 0;
        assert_ne!(
            unsafe { CryptGenKey(provider.handle, AT_SIGNATURE, 2048 << 16, &mut key) },
            0
        );
        assert_ne!(unsafe { CryptDestroyKey(key) }, 0);
        let mut subject = [0u8; 512];
        let mut subject_len = subject.len() as u32;
        assert_ne!(
            unsafe {
                CertStrToNameW(
                    X509_ASN_ENCODING,
                    w!("CN=off42 disposable signing test"),
                    CERT_X500_NAME_STR,
                    null(),
                    subject.as_mut_ptr(),
                    &mut subject_len,
                    null_mut(),
                )
            },
            0
        );
        let subject = CRYPT_INTEGER_BLOB {
            cbData: subject_len,
            pbData: subject.as_mut_ptr(),
        };
        let algorithm = CRYPT_ALGORITHM_IDENTIFIER {
            pszObjId: szOID_RSA_SHA256RSA.cast_mut(),
            ..Default::default()
        };
        let key_info = CRYPT_KEY_PROV_INFO {
            pwszContainerName: provider.name.as_ptr().cast_mut(),
            pwszProvName: MS_ENH_RSA_AES_PROV_W.cast_mut(),
            dwProvType: PROV_RSA_AES,
            dwKeySpec: AT_SIGNATURE,
            ..Default::default()
        };
        let cert = Certificate(unsafe {
            CertCreateSelfSignCertificate(
                provider.handle,
                &subject,
                0,
                &key_info,
                &algorithm,
                null(),
                null(),
                null(),
            )
        });
        assert!(
            !cert.0.is_null(),
            "creating disposable certificate: 0x{:08X}",
            unsafe { GetLastError() }
        );

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("unsigned.pdf");
        let mut pdf = Document::with_version("1.5");
        let pages = pdf.new_object_id();
        let page = pdf.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages, "MediaBox" => vec![0.into(), 0.into(), 300.into(), 200.into()],
        });
        pdf.objects.insert(
            pages,
            dictionary! {
                "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1,
            }
            .into(),
        );
        let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        pdf.trailer.set("Root", catalog);
        pdf.save(&path).unwrap();
        let original = std::fs::read(&path).unwrap();
        let doc = pdf42_core::PdfDocument::open(&path).unwrap();
        let draft = doc
            .capture_signature(true, "Board revision 42")
            .unwrap()
            .prepare(
                &super::subject(&cert),
                "D:20261109210000Z",
                "2026-11-09 21:00:00 UTC",
            )
            .unwrap();
        let cms = sign_cms(&cert, draft.signed_bytes()).unwrap();
        assert_eq!(cms_der(&cms), Some(cms.as_slice()));
        let mut signed = draft.finish(&cms).unwrap();
        let report = inspect_bytes(&signed, "signed.pdf").unwrap();
        assert_eq!(report.rows.len(), 1);
        assert_eq!(report.rows[0][1], "off42 disposable signing test");
        assert!(
            report.rows[0][2].starts_with("Signature intact. Certificate trust not established")
        );
        assert_eq!(report.rows[0][3], "Entire file");
        let parsed = load(&signed).unwrap();
        let signature_fields = fields(&parsed).unwrap();
        assert_eq!(signature_fields.len(), 1);
        assert!(parsed
            .objects
            .values()
            .filter_map(|object| object.as_dict().ok())
            .any(
                |node| node.get(b"FT").and_then(lopdf::Object::as_name).ok() == Some(b"Sig")
                    && node.get(b"AP").and_then(lopdf::Object::as_dict).is_ok()
            ));
        let appearance = b"0.97 0.98 0.99 rg";
        let position = signed
            .windows(appearance.len())
            .position(|window| window == appearance)
            .unwrap();
        signed[position + 2] = b'8';
        let changed = inspect_bytes(&signed, "changed.pdf").unwrap();
        assert!(changed.rows[0][2].starts_with("Signature verification failed"));
        assert_eq!(std::fs::read(path).unwrap(), original);
    }
}
