# Run with `brew ruby scripts/verify-nimbus-homebrew-install-steps.rb`.
require "install_steps"

root = File.expand_path("..", __dir__)
expected = [{
  "command" => { "path" => "/usr/bin/xattr" },
  "guards" => [{ "condition" => "on", "value" => "macos", "id" => "1" }],
  "type" => "run",
  "args" => ["-dr", "com.apple.quarantine", "{{staged_path}}"]
}]
[".github/workflows/release.yml", "scripts/collect-nimbus-homebrew-cask-proof.sh"].each do |path|
  source = File.read(File.join(root, path))
  match = source.match(/^([ ]*)postflight_steps do\n(.*?)^\1end$/m)
  abort "FAIL: missing structured hook in #{path}" unless match
  dsl = Homebrew::InstallSteps::DSL.new(default_base: :staged_path)
  dsl.instance_eval(match[2], path)
  abort "FAIL: changed install contract in #{path}" unless dsl.steps == expected
  puts "PASS: #{path} serializes macOS-only, required, unprivileged quarantine cleanup"
end
