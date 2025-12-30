# frozen_string_literal: true

require_relative 'test_helper'
require 'open3'
require 'tmpdir'

class HarnessTest < Minitest::Test
  HARNESS_DIR = File.expand_path('rust_harness', __dir__)

  def setup
    # Ensure harness is built
    unless File.exist?(File.join(HARNESS_DIR, 'target', 'release', 'run_parser'))
      system('cargo', 'build', '--release', chdir: HARNESS_DIR)
    end
  end

  def test_minimal_parser
    output = run_parser('examples/minimal.desc', "hello world\n")
    assert_equal ['Text "hello world\n" @ 0..12'], output
  end

  def test_lines_parser
    output = run_parser('examples/lines.desc', "line one\nline two\n")
    # lines.desc uses Text type, and includes the newline in the content
    assert_equal [
      'Text "line one\n" @ 0..9',
      'Text "line two\n" @ 9..18'
    ], output
  end

  def test_empty_input
    output = run_parser('examples/minimal.desc', '')
    assert_equal ['Text "" @ 0..0'], output
  end

  def test_elements_parser
    output = run_parser('examples/elements.desc', "|div Hello\n")
    assert_equal [
      'ElementStart @ 1..1',
      'Name "div" @ 1..4',
      'Text "Hello" @ 5..10',
      'ElementEnd @ 11..11'
    ], output
  end

  def test_nested_elements
    output = run_parser('examples/elements.desc', "|div\n  |span Hi\n")
    assert_equal [
      'ElementStart @ 1..1',
      'Name "div" @ 1..4',
      'ElementStart @ 8..8',
      'Name "span" @ 8..12',
      'Text "Hi" @ 13..15',
      'ElementEnd @ 16..16',
      'ElementEnd @ 16..16'
    ], output
  end

  private

  def run_parser(desc_file, input)
    desc_path = File.expand_path("../#{desc_file}", __dir__)

    # Generate parser
    parser_code = Descent.generate(desc_path, target: :rust)

    # Write to harness
    generated_path = File.join(HARNESS_DIR, 'src', 'generated.rs')
    File.write(generated_path, parser_code)

    # Build (release for speed)
    _, stderr, status = Open3.capture3(
      'cargo', 'build', '--release', '--quiet',
      chdir: HARNESS_DIR
    )
    raise "Build failed: #{stderr}" unless status.success?

    # Run parser
    binary = File.join(HARNESS_DIR, 'target', 'release', 'run_parser')
    stdout, stderr, status = Open3.capture3(binary, stdin_data: input)
    raise "Parser failed: #{stderr}" unless status.success?

    stdout.lines.map(&:chomp)
  end
end
