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
  version, checksum, path = ARGV
  abort "Usage: ruby scripts/update-cask.rb VERSION SHA256 CASK_PATH" unless ARGV.length == 3

  source = File.read(path)
  updated = updated_cask(source, version, checksum)
  if updated == source
    puts "Homebrew cask is already at this version or newer."
    exit
  end

  File.write(path, updated)
  puts "Homebrew cask updated to #{version}."
end
