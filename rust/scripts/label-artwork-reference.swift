// Independent native macOS reference for PDF region/rotation placement; no application state is changed.
import AppKit
import PDFKit
let directory=URL(fileURLWithPath:CommandLine.arguments[1],isDirectory:true)
let mm=72.0/25.4
for rotation in [0,90,180,270] {
    let source=try Data(contentsOf:directory.appendingPathComponent("source-\(rotation).pdf"))
    let pdf=PDFDocument(data:source)!
    let page=pdf.page(at:0)!
    page.setBounds(CGRect(x:30,y:40,width:100,height:200),for:.cropBox)
    let provider=CGDataProvider(data:pdf.dataRepresentation()! as CFData)!
    let artwork=CGPDFDocument(provider)!.page(at:1)!
    let data=NSMutableData();var media=CGRect(x:0,y:0,width:210*mm,height:297*mm)
    let context=CGContext(consumer:CGDataConsumer(data:data)!,mediaBox:&media,nil)!
    for slot in [23,0] {
        context.beginPDFPage(nil)
        let cell=CGRect(x:Double(slot%3)*70*mm,y:(297-0.5-Double(slot/3)*37-37)*mm,width:70*mm,height:37*mm)
        context.saveGState();context.clip(to:cell)
        context.concatenate(artwork.getDrawingTransform(.cropBox,rect:cell,rotate:0,preserveAspectRatio:true))
        context.drawPDFPage(artwork)
        context.restoreGState();context.endPDFPage()
    }
    context.closePDF()
    try (data as Data).write(to:directory.appendingPathComponent("reference-\(rotation).pdf"),options:.withoutOverwriting)
}
