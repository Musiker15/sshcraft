# Template: `cargo xtask stores SHA256SUMS.txt` fills it in (docs/releasing.md, "Stores").
cask "sshcraft" do
  version "{{version}}"
  sha256 "{{sha256:sshcraft-{{version}}-macos-universal.dmg}}"

  url "https://github.com/Musiker15/sshcraft/releases/download/v#{version}/sshcraft-#{version}-macos-universal.dmg"
  name "SSHCraft"
  desc "SSH client with end-to-end encrypted, self-hosted sync"
  homepage "https://github.com/Musiker15/sshcraft"

  livecheck do
    url :url
    strategy :github_latest
  end

  depends_on macos: ">= :big_sur"

  app "SSHCraft.app"

  # The data folder of sshcraft_platform::data_dir(): hosts, keys and settings.
  zap trash: "~/Library/Application Support/de.musiker15.SSHCraft"
end
