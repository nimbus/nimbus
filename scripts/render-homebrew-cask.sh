#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: render-homebrew-cask.sh --version <version> \
         --sha-darwin-arm64 <sha256> \
         --sha-linux-arm64 <sha256> \
         --sha-linux-x86_64 <sha256> \
         [--output <path>]

Render the shipped `nimbus` Homebrew cask. The release workflow calls this with
the checksums of the published release assets; CI calls it with placeholder
checksums and runs `brew style --cask` over the result, so a Homebrew release
that changes the canonical cask shape fails a pull request instead of shipping
a cask that `brew style` and `brew audit` reject.

Stanza order and grouping follow what `brew style --cask` autocorrects to. Do
not reorder by hand: render the cask, run `brew style --fix --cask` on it
inside a scratch tap, and copy the result back.

options:
  --version <version>            Cask version, without a leading "v"
  --sha-darwin-arm64 <sha256>    SHA-256 of nimbus_darwin_arm64.tar.gz
  --sha-linux-arm64 <sha256>     SHA-256 of nimbus_linux_arm64.tar.gz
  --sha-linux-x86_64 <sha256>    SHA-256 of nimbus_linux_x86_64.tar.gz
  --output <path>                Write here instead of stdout
  -h, --help                     Show this help
EOF
}

version=""
sha_darwin_arm64=""
sha_linux_arm64=""
sha_linux_x86_64=""
output=""

while [ "$#" -gt 0 ]; do
  case "$1" in
    --version) version="${2:?missing version}"; shift 2 ;;
    --sha-darwin-arm64) sha_darwin_arm64="${2:?missing sha}"; shift 2 ;;
    --sha-linux-arm64) sha_linux_arm64="${2:?missing sha}"; shift 2 ;;
    --sha-linux-x86_64) sha_linux_x86_64="${2:?missing sha}"; shift 2 ;;
    --output) output="${2:?missing path}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

if [ -z "${version}" ]; then
  echo "--version is required" >&2
  exit 2
fi

# Assert each checksum in the renderer rather than at the call site. A loose
# lookup upstream can yield an empty string or several lines, and either would
# splice a cask that installs the wrong bytes or does not parse.
require_sha256() {
  if [[ ! "$2" =~ ^[0-9a-f]{64}$ ]]; then
    echo "$1 is not a single SHA-256 (got: '$2')" >&2
    exit 1
  fi
}
require_sha256 --sha-darwin-arm64 "${sha_darwin_arm64}"
require_sha256 --sha-linux-arm64 "${sha_linux_arm64}"
require_sha256 --sha-linux-x86_64 "${sha_linux_x86_64}"

render() {
  # `#{version}` is Ruby interpolation evaluated by Homebrew, not by the shell:
  # it carries no `$`, so an unquoted heredoc leaves it alone while still
  # expanding the shell variables below.
  cat <<CASKEOF
cask "nimbus" do
  version "${version}"

  on_macos do
    on_arm do
      sha256 "${sha_darwin_arm64}"
      url "https://github.com/nimbus/nimbus/releases/download/v#{version}/nimbus_darwin_arm64.tar.gz"
    end
    # Intel Macs are not a release target: there is no nimbus_darwin_x86_64
    # asset, so the requirement is declared rather than left to a 404.
    depends_on arch: :arm64
    depends_on macos: :sonoma
  end
  on_linux do
    on_arm do
      sha256 "${sha_linux_arm64}"
      url "https://github.com/nimbus/nimbus/releases/download/v#{version}/nimbus_linux_arm64.tar.gz"
    end
    on_intel do
      sha256 "${sha_linux_x86_64}"
      url "https://github.com/nimbus/nimbus/releases/download/v#{version}/nimbus_linux_x86_64.tar.gz"
    end
  end

  name "nimbus"
  desc "Self-hosted JavaScript backend runtime powered by V8"
  homepage "https://github.com/nimbus/nimbus"

  livecheck do
    skip "Auto-generated on release."
  end

  # No depends_on formula for the krunkit chain: krunkit lives in the
  # third-party libkrun/krun tap, and tap trust is non-transitive, so a
  # nimbus/tap cask can never pre-trust another tap on the user's behalf. The
  # chain is optional and the caveats below explain it.
  binary "nimbus"

  # Homebrew quarantines the downloaded archive and extraction propagates the
  # attribute onto the unsigned binary, so Gatekeeper would kill it on first
  # run. There is no cask-side opt-out; \`--no-quarantine\` is the user's flag,
  # not the cask's. \`xattr -dr\` exits 0 whether or not the attribute is
  # present, so this is also correct under \`--no-quarantine\`.
  #
  # \`postflight_steps\` is the structured replacement for the legacy Ruby
  # flight blocks and needs Homebrew 5.1.14 or newer. An older Homebrew reports
  # "Unexpected method 'postflight_steps'" and installs without running it;
  # Homebrew self-updates, and tap trust already requires 6.0, so that window
  # is narrow.
  postflight_steps do
    on_macos do
      run "/usr/bin/xattr", args: ["-dr", "com.apple.quarantine", "{{staged_path}}"], sudo: false, must_succeed: true
    end
  end

  caveats <<~EOS
    Nimbus is installed. Quick start:
      nimbus --help              # Show all commands
      nimbus start               # Start the server

    Optional macOS microVM dev flow ('nimbus machine'):
    it needs krunkit from the third-party libkrun/krun tap. Installing krunkit
    by its full name trusts krunkit alone, and the install then stops at the
    gvproxy dependency in that same tap, so trust the whole tap first:

      brew trust libkrun/krun
      brew install libkrun/krun/krunkit

    The 'nimbus' server itself runs fine without this chain.

    A pinned vfkit (Apple Virtualization.framework) ships bundled in
    this cask as an opt-in machine backend; enable it with
    NIMBUS_MACHINE_PROVIDER=vfkit. The default backend stays krunkit.

    Documentation: https://github.com/nimbus/nimbus
  EOS
end
CASKEOF
}

if [ -n "${output}" ]; then
  render > "${output}"
else
  render
fi
