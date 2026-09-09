const repository = "https://github.com/zoeyrose/velvet-scroll";
const distribution = document.querySelector("#distribution");
const architecture = document.querySelector("#architecture");
const download = document.querySelector("#download-link");
const status = document.querySelector("#download-status");
const command = document.querySelector("#install-command");
const copy = document.querySelector("#copy-command");
const note = document.querySelector("#package-note");
let release;

const formats = {
  deb: {
    command: "sudo apt install ./velvet-scroll_*.deb",
    label: "Debian / Ubuntu package",
    note: "Use one downloaded package matching your architecture. Builds require glibc 2.39 or newer.",
  },
  rpm: {
    command: "sudo dnf install ./velvet-scroll-*.rpm",
    label: "Fedora package",
    note: "Built for Fedora package names. Other RPM distributions may need different Qt dependencies.",
  },
  arch: {
    command: "sudo pacman -U ./velvet-scroll-*.pkg.tar.zst",
    label: "Arch Linux package",
    note: "Installs a package tracked by pacman. No AUR helper is needed.",
  },
  archive: {
    command:
      "tar -xzf velvet-scroll-*-linux-*.tar.gz\ncd velvet-scroll-*-linux-*\nsudo ./install.sh",
    label: "Linux archive",
    note: "Requires glibc 2.39+, Python 3, PyQt6 and Qt SVG support. See the guide for source builds on older systems.",
  },
};

function expectedName(version, format, arch) {
  if (format === "deb")
    return `velvet-scroll_${version}_${arch === "x86_64" ? "amd64" : "arm64"}.deb`;
  if (format === "rpm") return `velvet-scroll-${version}-1.${arch}.rpm`;
  if (format === "arch")
    return `velvet-scroll-${version}-1-${arch}.pkg.tar.zst`;
  return `velvet-scroll-${version}-linux-${arch}.tar.gz`;
}

function updateDownload() {
  const format = formats[distribution.value];
  command.textContent = format.command;
  note.textContent = format.note;
  copy.textContent = "Copy";
  download.href = `${repository}/releases/latest`;
  download.textContent = "Choose a package on GitHub ↗";
  if (!release) return;
  const version = release.tag_name.slice(1);
  const name = expectedName(version, distribution.value, architecture.value);
  const asset = release.assets.find((item) => item.name === name);
  // Build the URL from the verified tag and exact expected name, never API HTML.
  if (asset) {
    download.href = `${repository}/releases/download/${encodeURIComponent(release.tag_name)}/${encodeURIComponent(name)}`;
    download.textContent = `Download ${format.label} ↓`;
    status.textContent = `${version} · ${architecture.value === "aarch64" ? "ARM64" : "x86_64"}${Number.isFinite(asset.size) ? ` · ${(asset.size / 1024 / 1024).toFixed(1)} MB` : ""}`;
  } else {
    status.textContent =
      "This package is not listed yet. Check the release page for available downloads.";
  }
}

distribution.addEventListener("change", updateDownload);
architecture.addEventListener("change", updateDownload);
if (navigator.clipboard?.writeText) {
  copy.hidden = false;
  copy.addEventListener("click", async () => {
    try {
      await navigator.clipboard.writeText(command.textContent);
      copy.textContent = "Copied";
    } catch {
      copy.textContent = "Select the command to copy";
    }
  });
}
updateDownload();

async function loadRelease() {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), 6000);
  try {
    const response = await fetch(
      "https://api.github.com/repos/zoeyrose/velvet-scroll/releases/latest",
      {
        signal: controller.signal,
        headers: { Accept: "application/vnd.github+json" },
      },
    );
    if (!response.ok) throw new Error("Release unavailable");
    const data = await response.json();
    if (
      !/^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(data.tag_name) ||
      data.draft ||
      data.prerelease ||
      !Array.isArray(data.assets)
    )
      throw new Error("Invalid stable release");
    release = data;
    document.querySelector("#release-label").textContent =
      `Version ${data.tag_name.slice(1)}`;
    updateDownload();
  } catch {
    status.textContent = "View available packages on GitHub Releases.";
  } finally {
    clearTimeout(timeout);
  }
}
loadRelease();
