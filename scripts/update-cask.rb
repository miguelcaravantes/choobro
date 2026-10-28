require "base64"
require "json"
require "open3"
require "rubygems"

def updated_cask(source, version, checksum)
  raise ArgumentError, "Invalid release version" unless version.match?(/\A\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?\z/)
  raise ArgumentError, "Invalid SHA-256 checksum" unless checksum.match?(/\A[0-9a-f]{64}\z/)

  versions = source.scan(/^  version "([^"]+)"$/).flatten
  checksums = source.scan(/^  sha256 "([0-9a-f]{64})"$/).flatten
  raise ArgumentError, "Expected one cask version and checksum" unless versions.length == 1 && checksums.length == 1

  current = Gem::Version.new(versions.first)
  incoming = Gem::Version.new(version)
  return source if incoming < current
  if incoming == current
    raise ArgumentError, "Refusing to change a published version's checksum" unless checksum == checksums.first
    return source
  end

  source.sub(/^  version "[^"]+"$/, "  version \"#{version}\"")
        .sub(/^  sha256 "[0-9a-f]{64}"$/, "  sha256 \"#{checksum}\"")
end

if $PROGRAM_NAME == __FILE__
  version, checksum = ARGV
  abort "Usage: ruby scripts/update-cask.rb VERSION SHA256" unless ARGV.length == 2

  path = "repos/#{ENV.fetch("GH_REPO")}/contents/Casks/choobro.rb"
  output, error, status = Open3.capture3("gh", "api", "#{path}?ref=main")
  abort error unless status.success?
  file = JSON.parse(output)
  source = Base64.strict_decode64(file.fetch("content").delete("\n"))
  updated = updated_cask(source, version, checksum)
  if updated == source
    puts "Homebrew cask is already at this version or newer."
    exit
  end

  payload = JSON.generate(
    message: "Update Homebrew cask to #{version}",
    branch: "main",
    sha: file.fetch("sha"),
    content: Base64.strict_encode64(updated)
  )
  output, error, status = Open3.capture3("gh", "api", "--method", "PUT", path, "--input", "-", stdin_data: payload)
  abort error unless status.success?
  puts "Homebrew cask updated: #{JSON.parse(output).fetch("commit").fetch("html_url")}"
end
