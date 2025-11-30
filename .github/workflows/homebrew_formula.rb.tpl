class FORMULA_CLASS < Formula
  desc "FORMULA_DESC"
  homepage "FORMULA_HOME"
  version "FORMULA_VER"

  on_macos do
    if Hardware::CPU.arm?
      url "URL_ARM"
      sha256 "SHA_ARM"
    else
      url "URL_X64"
      sha256 "SHA_X64"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "URL_LINUX_ARM64"
      sha256 "SHA_LINUX_ARM64"
    else
      url "URL_LINUX_X64"
      sha256 "SHA_LINUX_X64"
    end
  end

  def install
FORMULA_INSTALLS
  end

  test do
    assert_match "OK", "OK"
  end
end
