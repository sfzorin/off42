import eu.europa.esig.dss.model.FileDocument;
import eu.europa.esig.dss.spi.DSSUtils;
import eu.europa.esig.dss.spi.validation.CommonCertificateVerifier;
import eu.europa.esig.dss.spi.x509.CommonTrustedCertificateSource;
import eu.europa.esig.dss.validation.SignedDocumentValidator;
import eu.europa.esig.dss.validation.reports.Reports;
import eu.europa.esig.dss.simplereport.SimpleReport;
import java.io.*;
import java.nio.file.*;

/** Offline validation with EU DSS: only the validation data embedded in the PDF is used. */
public class Validate {
    public static void main(String[] a) throws Exception {
        CommonTrustedCertificateSource trusted = new CommonTrustedCertificateSource();
        String pem = Files.readString(Path.of(a[0]));
        for (String block : pem.split("-----END CERTIFICATE-----")) {
            int i = block.indexOf("-----BEGIN CERTIFICATE-----");
            if (i < 0) continue;
            try { trusted.addCertificate(DSSUtils.loadCertificate((block.substring(i) + "-----END CERTIFICATE-----\n").getBytes())); } catch (Exception e) {}
        }
        for (int k = 1; k < a.length; k++) {
            CommonCertificateVerifier cv = new CommonCertificateVerifier();
            cv.setTrustedCertSources(trusted);
            cv.setOcspSource(null); cv.setCrlSource(null); cv.setAIASource(null);
            SignedDocumentValidator v = SignedDocumentValidator.fromDocument(new FileDocument(a[k]));
            v.setCertificateVerifier(cv);
            Reports r = v.validateDocument();
            SimpleReport s = r.getSimpleReport();
            System.out.println("== " + a[k]);
            for (String id : s.getSignatureIdList()) {
                System.out.println("  signature " + s.getSignedBy(id) + " | format=" + s.getSignatureFormat(id) + " | " + s.getIndication(id)
                    + (s.getSubIndication(id) != null ? "/" + s.getSubIndication(id) : "") + " | qualification=" + s.getSignatureQualification(id));
                s.getAdESValidationErrors(id).forEach(m -> System.out.println("    error: " + m.getValue()));
                s.getAdESValidationWarnings(id).forEach(m -> System.out.println("    warning: " + m.getValue()));
            }
            for (var t : r.getDiagnosticData().getTimestampList()) {
                System.out.println("  timestamp " + t.getType() + " | " + t.getProductionTime() + " | imprint intact=" + t.isMessageImprintDataIntact()
                    + " | signature intact=" + t.isSignatureIntact() + " | " + s.getIndication(t.getId()));
            }
            if (System.getenv("DSS_REPORTS") != null) { Files.writeString(Path.of(a[k] + ".detailed.xml"), r.getXmlDetailedReport()); Files.writeString(Path.of(a[k] + ".diag.xml"), r.getXmlDiagnosticData()); }
        }
    }
}
