// Metadata-only owned fixtures: proves registration + strict signing, not plugin ABI/playback.
import AppKit
import CoreServices
import Foundation
import UniformTypeIdentifiers

func require(_ value: Bool, _ message: String) {
    guard value else { fputs(message + "\n", stderr); exit(1) }
}
func run(_ command: String, _ arguments: [String]) throws {
    let p = Process(); p.executableURL = URL(fileURLWithPath: command); p.arguments = arguments
    try p.run(); p.waitUntilExit(); require(p.terminationStatus == 0, "Failed: \(command)")
}
let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
defer { try? FileManager.default.removeItem(at: root) }
let c = root.appendingPathComponent("owned.c")
try "int main(void) { return 0; }\n".write(to: c, atomically: true, encoding: .utf8)
let executable = root.appendingPathComponent("owned")
try run("/usr/bin/xcrun", ["clang", c.path, "-o", executable.path])
var plugins: [URL] = []
func bundle(_ name: String, _ type: String, _ extra: [String: Any] = [:]) throws -> URL {
    let url = root.appendingPathComponent(name)
    try FileManager.default.createDirectory(at: url.appendingPathComponent("Contents/MacOS"), withIntermediateDirectories: true)
    try FileManager.default.copyItem(at: executable, to: url.appendingPathComponent("Contents/MacOS/owned"))
    var info: [String: Any] = ["CFBundleExecutable": "owned", "CFBundleIdentifier": "audio.matari.kontra.preflight." + name.replacingOccurrences(of: ".", with: "-"), "CFBundlePackageType": type, "CFBundleVersion": "1", "CFBundleName": "Owned fixture"]
    info.merge(extra) { _, new in new }
    try PropertyListSerialization.data(fromPropertyList: info, format: .xml, options: 0).write(to: url.appendingPathComponent("Contents/Info.plist"))
    try run("/usr/bin/codesign", ["--force", "--sign", "-", url.path])
    try run("/usr/bin/codesign", ["--verify", "--deep", "--strict", url.path])
    return url
}
for ext in ["clap", "vst3"] { plugins.append(try bundle("Owned." + ext, "BNDL")) }
func inspect(_ url: URL, _ phase: String) throws -> Bool {
    var fresh = URL(fileURLWithPath: url.path); fresh.removeAllCachedResourceValues()
    let values = try fresh.resourceValues(forKeys: [.isPackageKey, .contentTypeKey])
    let workspace = NSWorkspace.shared.isFilePackage(atPath: url.path)
    let packageType = values.contentType?.conforms(to: .package) ?? false
    let object: [String: Any] = ["phase": phase, "extension": url.pathExtension, "is_package": values.isPackage ?? false, "workspace_package": workspace, "package_uti": packageType, "content_type": values.contentType?.identifier ?? "missing"]
    print(String(data: try JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]), encoding: .utf8)!)
    return values.isPackage == true && workspace && packageType
}
for url in plugins { _ = try inspect(url, "before-registration") }
let infoProcess = Process(); infoProcess.executableURL = URL(fileURLWithPath: "/usr/bin/env")
infoProcess.arguments = ["python3", "-c", "import json,sys; sys.path.insert(0,'.github/scripts'); from package_macos import app_info; print(json.dumps(app_info('0.3.115-nightly.preflight')))"]
let infoPipe = Pipe(); infoProcess.standardOutput = infoPipe
try infoProcess.run(); infoProcess.waitUntilExit(); require(infoProcess.terminationStatus == 0, "Production package declaration generation failed")
let appInfo = try JSONSerialization.jsonObject(with: infoPipe.fileHandleForReading.readDataToEndOfFile()) as! [String: Any]
let registrar = try bundle("KONTRA.app", "APPL", ["UTImportedTypeDeclarations": appInfo["UTImportedTypeDeclarations"]!, "CFBundleDocumentTypes": appInfo["CFBundleDocumentTypes"]!])

require(LSRegisterURL(registrar as CFURL, true) == noErr, "Owned application registration failed")
for url in plugins {
    require(try inspect(url, "after-registration"), "Native package recognition failed: " + url.path)
    try run("/usr/bin/codesign", ["--verify", "--deep", "--strict", url.path])
    try run("/usr/bin/xattr", ["-r", "-l", url.path])
}
try run("/usr/bin/codesign", ["--verify", "--deep", "--strict", registrar.path])
print("NATIVE_OWNED_PACKAGE_PREFLIGHT_PASS; ad-hoc fixture signatures only; no FinderInfo writes")
