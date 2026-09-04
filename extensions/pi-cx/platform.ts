export interface PlatformConfig {
  platform: NodeJS.Platform;
  arch: NodeJS.Architecture;
  target: string;
  platformDir: string;
  binaryName: string;
  grammarPrefix: string;
  grammarExtension: "dylib" | "so" | "dll";
}

export const SUPPORTED_PLATFORMS: readonly PlatformConfig[] = [
  { platform: "darwin", arch: "arm64", target: "aarch64-apple-darwin", platformDir: "darwin-arm64", binaryName: "cx", grammarPrefix: "lib", grammarExtension: "dylib" },
  { platform: "darwin", arch: "x64", target: "x86_64-apple-darwin", platformDir: "darwin-x64", binaryName: "cx", grammarPrefix: "lib", grammarExtension: "dylib" },
  { platform: "linux", arch: "arm64", target: "aarch64-unknown-linux-gnu", platformDir: "linux-arm64", binaryName: "cx", grammarPrefix: "lib", grammarExtension: "so" },
  { platform: "linux", arch: "x64", target: "x86_64-unknown-linux-gnu", platformDir: "linux-x64", binaryName: "cx", grammarPrefix: "lib", grammarExtension: "so" },
  { platform: "win32", arch: "arm64", target: "aarch64-pc-windows-msvc", platformDir: "win32-arm64", binaryName: "cx.exe", grammarPrefix: "", grammarExtension: "dll" },
  { platform: "win32", arch: "x64", target: "x86_64-pc-windows-msvc", platformDir: "win32-x64", binaryName: "cx.exe", grammarPrefix: "", grammarExtension: "dll" },
] as const;

export function currentPlatformConfig(platform = process.platform, arch = process.arch): PlatformConfig {
  const config = SUPPORTED_PLATFORMS.find((item) => item.platform === platform && item.arch === arch);
  if (!config) throw new Error(`pi-cx does not support ${platform}/${arch}; supported: ${SUPPORTED_PLATFORMS.map((item) => `${item.platform}/${item.arch}`).join(", ")}`);
  return config;
}

export function grammarFilename(name: string, config = currentPlatformConfig()): string {
  return `${config.grammarPrefix}tree_sitter_${name}.${config.grammarExtension}`;
}
