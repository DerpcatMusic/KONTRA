// Native discovery gate, not a Bitwig/DAW playback test. Run with xcrun swift.
import AppKit
import CoreFoundation
import CoreServices
import Darwin
import Foundation
import UniformTypeIdentifiers

func require(_ condition: Bool, _ message: String) {
    guard condition else { fputs(message + "\n", stderr); exit(1) }
}

require(CommandLine.arguments.count == 5 && CommandLine.arguments[1] == "--register", "Pass --register KONTRA.app CLAP VST3 paths")
let registrar = URL(fileURLWithPath: CommandLine.arguments[2]).standardizedFileURL
require(Bundle(url: registrar)?.infoDictionary?["CFBundlePackageType"] as? String == "APPL", "Missing owned application")
require(LSRegisterURL(registrar as CFURL, true) == noErr, "Application type registration failed")
for path in CommandLine.arguments.dropFirst(3) {
    var url = URL(fileURLWithPath: path).standardizedFileURL
    url.removeAllCachedResourceValues()
    let values = try url.resourceValues(forKeys: [.isPackageKey, .contentTypeKey])
    require(values.isPackage == true, "Finder does not recognize a package: \(path)")
    require(NSWorkspace.shared.isFilePackage(atPath: url.path), "Workspace does not recognize a package: \(path)")
    guard let type = values.contentType else { fatalError("Missing native UTType: \(path)") }
    require(type.conforms(to: .package), "Native type does not conform to package: \(type.identifier)")
    let forbidden = url.withUnsafeFileSystemRepresentation { getxattr($0!, "com.apple.FinderInfo", nil, 0, 0, XATTR_NOFOLLOW) }
    require(forbidden == -1 && errno == ENOATTR, "FinderInfo detritus on signed bundle: \(path)")
    guard let bundle = Bundle(url: url), let executable = bundle.executableURL else { fatalError("Not a loadable bundle: \(path)") }
    require(bundle.infoDictionary?["CFBundlePackageType"] as? String == "BNDL", "Wrong plug-in package type: \(path)")
    guard let module = dlopen(executable.path, RTLD_NOW | RTLD_LOCAL) else { fatalError(String(cString: dlerror())) }
    defer { dlclose(module) }
    var classes: UInt32 = 0
    if url.pathExtension == "clap" {
        guard let entry = dlsym(module, "clap_entry") else { fatalError("Missing CLAP entry") }
        require(entry.load(as: UInt32.self) == 1, "Unsupported CLAP ABI")
        typealias Init = @convention(c) (UnsafePointer<CChar>?) -> Bool
        typealias Deinit = @convention(c) () -> Void
        typealias Factory = @convention(c) (UnsafePointer<CChar>?) -> UnsafeRawPointer?
        typealias Count = @convention(c) (UnsafeRawPointer?) -> UInt32
        typealias Descriptor = @convention(c) (UnsafeRawPointer?, UInt32) -> UnsafeRawPointer?
        let initialize = unsafeBitCast(entry.advanced(by: 16).load(as: UnsafeRawPointer.self), to: Init.self)
        let deinitialize = unsafeBitCast(entry.advanced(by: 24).load(as: UnsafeRawPointer.self), to: Deinit.self)
        let factory = unsafeBitCast(entry.advanced(by: 32).load(as: UnsafeRawPointer.self), to: Factory.self)
        require(url.path.withCString { initialize($0) }, "CLAP initialization failed")
        defer { deinitialize() }
        guard let plugins = "clap.plugin-factory".withCString({ factory($0) }) else { fatalError("Missing CLAP factory") }
        let count = unsafeBitCast(plugins.load(as: UnsafeRawPointer.self), to: Count.self)
        let descriptor = unsafeBitCast(plugins.advanced(by: 8).load(as: UnsafeRawPointer.self), to: Descriptor.self)
        classes = count(plugins); require(classes > 0, "CLAP factory is empty")
        guard let info = descriptor(plugins, 0) else { fatalError("Missing CLAP descriptor") }
        let name = String(cString: info.advanced(by: 24).load(as: UnsafePointer<CChar>.self))
        let version = String(cString: info.advanced(by: 64).load(as: UnsafePointer<CChar>.self))
        require(name == "KONTRA" && version == (bundle.infoDictionary?["KONTRAVersion"] as? String), "CLAP identity differs from bundle")
    } else {
        require(url.pathExtension == "vst3", "Unexpected plug-in format")
        typealias Entry = @convention(c) (UnsafeRawPointer?) -> Bool
        typealias Exit = @convention(c) () -> Bool
        typealias Factory = @convention(c) () -> UnsafeRawPointer?
        typealias Method = @convention(c) (UnsafeRawPointer?) -> UInt32
        guard let entry = dlsym(module, "bundleEntry"), let exit = dlsym(module, "bundleExit"),
              let factory = dlsym(module, "GetPluginFactory"), let cf = CFBundleCreate(nil, url as CFURL) else { fatalError("Missing VST3 bundle exports") }
        require(unsafeBitCast(entry, to: Entry.self)(UnsafeRawPointer(Unmanaged.passUnretained(cf).toOpaque())), "VST3 bundle entry failed")
        defer { _ = unsafeBitCast(exit, to: Exit.self)() }
        guard let plugins = unsafeBitCast(factory, to: Factory.self)() else { fatalError("Missing VST3 factory") }
        let methods = plugins.load(as: UnsafePointer<UnsafeRawPointer>.self)
        classes = unsafeBitCast(methods[4], to: Method.self)(plugins)
        _ = unsafeBitCast(methods[2], to: Method.self)(plugins)
        require(classes > 0, "VST3 factory is empty")
    }
    let result: [String: Any] = ["bundle": url.path, "finder_info_absent": true, "package_uti": true,
        "is_package": true, "workspace_package": true, "content_type": type.identifier,
        "package_type": "BNDL", "factory_classes": classes]
    print(String(data: try JSONSerialization.data(withJSONObject: result, options: [.sortedKeys]), encoding: .utf8)!)
}
