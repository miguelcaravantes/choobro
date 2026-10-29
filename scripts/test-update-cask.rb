require_relative "update-cask"
require "open3"
require "tmpdir"

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

Dir.mktmpdir("choobro-cask-") do |directory|
  path = File.join(directory, "choobro.rb")
  File.write(path, source)
  script = File.join(__dir__, "update-cask.rb")
  output, error, status = Open3.capture3(RbConfig.ruby, script, "0.2.0", checksum, path)
  raise "Cask file update failed: #{output}#{error}" unless status.success?
  raise "Cask file was not updated" unless File.read(path) == updated
  _, _, status = Open3.capture3(RbConfig.ruby, script, "0.2.0", checksum, path)
  raise "Identical file update failed" unless status.success? && File.read(path) == updated
  _, _, status = Open3.capture3(RbConfig.ruby, script, "0.1.0", "a" * 64, path)
  raise "Downgraded the cask file" unless status.success? && File.read(path) == updated
  _, _, status = Open3.capture3(RbConfig.ruby, script, "0.2.0", "a" * 64, path)
  raise "Changed a published checksum" if status.success? || File.read(path) != updated
  _, _, status = Open3.capture3(RbConfig.ruby, script)
  raise "Accepted missing arguments" if status.success?
end

puts "Homebrew cask update checks passed."
