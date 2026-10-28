require_relative "update-cask"

source = "cask \"choobro\" do\n  version \"0.1.0\"\n  sha256 \"#{'a' * 64}\"\nend\n"
checksum = "b" * 64
updated = updated_cask(source, "0.2.0", checksum)
raise "Version was not updated" unless updated.include?('version "0.2.0"')
raise "Checksum was not updated" unless updated.include?("sha256 \"#{checksum}\"")
raise "Downgraded the cask" unless updated_cask(updated, "0.1.0", "a" * 64) == updated
raise "Changed an identical release" unless updated_cask(updated, "0.2.0", checksum) == updated
raise "Failed prerelease ordering" unless updated_cask(updated, "0.2.0-beta.1", "a" * 64) == updated

[
  [source, "0.1.0", checksum],
  [source, "invalid", checksum],
  [source, "0.2.0", "invalid"],
  ["", "0.2.0", checksum],
  [source + source, "0.2.0", checksum]
].each do |arguments|
  rejected = false
  begin
    updated_cask(*arguments)
  rescue ArgumentError
    rejected = true
  end
  raise "Accepted invalid cask update" unless rejected
end

puts "Homebrew cask update checks passed."
