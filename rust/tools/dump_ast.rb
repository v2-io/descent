# frozen_string_literal: true

# Dump Ruby descent's AST as canonical JSON for differential testing against
# `descent-rs ast`. Shape must stay in lockstep with
# rust/libdescent/src/dump.rs (see kind_to_json there for the value shapes).
#
# Usage: ruby -I lib rust/tools/dump_ast.rb FILE.desc

require 'descent'
require 'json'

module AstDump
  module_function

  def machine(m)
    {
      name: m.name,
      entry_point: m.entry_point,
      types: m.types.map { |t| { name: t.name, kind: t.kind.to_s, lineno: t.lineno } },
      functions: m.functions.map { |f| function(f) },
      keywords: m.keywords.map { |k| keywords(k) }
    }
  end

  def function(f)
    {
      name: f.name,
      return_type: f.return_type,
      params: f.params,
      states: f.states.map { |s| state(s) },
      eof_handler: f.eof_handler && eof(f.eof_handler),
      entry_actions: f.entry_actions.map { |c| command(c) },
      lineno: f.lineno
    }
  end

  def state(s)
    {
      name: s.name,
      cases: s.cases.map { |c| kase(c) },
      eof_handler: s.eof_handler && eof(s.eof_handler),
      lineno: s.lineno
    }
  end

  def kase(c)
    {
      chars: c.chars,
      condition: c.condition,
      substate: c.substate,
      commands: c.commands.map { |cmd| command(cmd) },
      lineno: c.lineno
    }
  end

  def eof(e)
    { commands: e.commands.map { |c| command(c) }, lineno: e.lineno }
  end

  def command(cmd)
    case cmd
    when Descent::AST::Conditional
      {
        node: 'conditional',
        clauses: cmd.clauses.map { |cl|
          { condition: cl.condition, commands: cl.commands.map { |c| command(c) } }
        },
        lineno: cmd.lineno
      }
    else
      { node: 'command', type: cmd.type.to_s, value: value(cmd.value), lineno: cmd.lineno }
    end
  end

  # Hash values ({var:, expr:} / {type:, literal:}) pass through; JSON
  # stringifies the symbol keys identically to the Rust side.
  def value(v) = v

  def keywords(k)
    {
      name: k.name,
      fallback: k.fallback,
      mappings: k.mappings.map { |m| { keyword: m[:keyword], event_type: m[:event_type] } },
      lineno: k.lineno
    }
  end
end

path = ARGV[0] or abort 'usage: dump_ast.rb FILE.desc'
content = File.read(path)
tokens = Descent::Lexer.new(content, source_file: path).tokenize
machine = Descent::Parser.new(tokens).parse
puts JSON.pretty_generate(AstDump.machine(machine))
