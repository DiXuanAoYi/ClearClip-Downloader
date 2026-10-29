# Homebrew Cask：放到自己的 tap（如 DiXuanAoYi/homebrew-tap 的 Casks/ 目录）后用
#   brew install --cask dixuanaoyi/tap/clearclip
cask "clearclip" do
  arch arm: "aarch64", intel: "x64"

  version "@@version@@"
  sha256 arm:   "@@sha_mac_arm@@",
         intel: "@@sha_mac_intel@@"

  url "https://github.com/DiXuanAoYi/ClearClip-Downloader/releases/download/desktop-v#{version}/ClearClip_#{version}_#{arch}.dmg"
  name "ClearClip"
  name "清影"
  desc "视频和图片网站作品解析与下载"
  homepage "https://github.com/DiXuanAoYi/ClearClip-Downloader"

  depends_on macos: ">= :catalina"

  app "ClearClip.app"

  zap trash: [
    "~/Library/Application Support/com.dixuanaoyi.clearclip",
    "~/Library/Caches/com.dixuanaoyi.clearclip",
    "~/Library/Logs/com.dixuanaoyi.clearclip",
  ]
end
