export const platforms = [
  { platform: "darwin", arch: "arm64", target: "aarch64-apple-darwin", platformDir: "darwin-arm64", bundleKey: "macos-arm64", binaryName: "cx", grammarPrefix: "lib", grammarExtension: "dylib" },
  { platform: "darwin", arch: "x64", target: "x86_64-apple-darwin", platformDir: "darwin-x64", bundleKey: "macos-x86_64", binaryName: "cx", grammarPrefix: "lib", grammarExtension: "dylib" },
  { platform: "linux", arch: "arm64", target: "aarch64-unknown-linux-gnu", platformDir: "linux-arm64", bundleKey: "linux-aarch64", binaryName: "cx", grammarPrefix: "lib", grammarExtension: "so" },
  { platform: "linux", arch: "x64", target: "x86_64-unknown-linux-gnu", platformDir: "linux-x64", bundleKey: "linux-x86_64", binaryName: "cx", grammarPrefix: "lib", grammarExtension: "so" },
  { platform: "win32", arch: "arm64", target: "aarch64-pc-windows-msvc", platformDir: "win32-arm64", bundleKey: "windows-aarch64", binaryName: "cx.exe", grammarPrefix: "", grammarExtension: "dll" },
  { platform: "win32", arch: "x64", target: "x86_64-pc-windows-msvc", platformDir: "win32-x64", bundleKey: "windows-x86_64", binaryName: "cx.exe", grammarPrefix: "", grammarExtension: "dll" },
];
export const grammarNames = ["rust", "typescript", "tsx", "python", "go", "c", "cpp", "markdown"];
export const languagePackVersion = "1.16.1";
export function byTarget(target) { const config = platforms.find(item => item.target === target); if (!config) throw new Error(`unsupported target ${target}`); return config; }
export function byHost(platform = process.platform, arch = process.arch) { const config = platforms.find(item => item.platform === platform && item.arch === arch); if (!config) throw new Error(`pi-cx does not support ${platform}/${arch}; supported: ${platforms.map(item => `${item.platform}/${item.arch}`).join(", ")}`); return config; }
export function grammarFilename(name, config) { return `${config.grammarPrefix}tree_sitter_${name}.${config.grammarExtension}`; }
