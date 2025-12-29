# frozen_string_literal: true

require_relative 'lib/descent/version'

Gem::Specification.new do |spec|
  spec.name    = 'descent'
  spec.version = Descent::VERSION
  spec.authors = ['Joseph Wecker']
  spec.email   = ['joseph.wecker@gmail.com']

  spec.summary     = 'Recursive descent parser generator from .desc specifications'
  spec.description = <<~DESC
    Generates high-performance callback-based recursive descent parsers from
    declarative .desc specifications. Supports multiple target languages (Rust, C)
    via Liquid templates. The .desc format is valid UDON, enabling future
    bootstrapping where descent can parse its own input format.
  DESC
  spec.homepage = 'https://github.com/josephwecker/descent'
  spec.license  = 'MIT'

  spec.required_ruby_version = '>= 3.3.0'

  spec.metadata['homepage_uri']          = spec.homepage
  spec.metadata['source_code_uri']       = spec.homepage
  spec.metadata['changelog_uri']         = "#{spec.homepage}/blob/main/CHANGELOG.md"
  spec.metadata['rubygems_mfa_required'] = 'true'

  spec.files = Dir.chdir(__dir__) do
    `git ls-files -z`.split("\x0").reject do |f|
      (File.expand_path(f) == __FILE__) ||
        f.start_with?(*%w[test/ .git .github Gemfile])
    end
  end

  spec.bindir        = 'exe'
  spec.executables   = ['descent']
  spec.require_paths = ['lib']

  spec.add_dependency 'liquid', '~> 5.0'
end
