# frozen_string_literal: true

desc 'Debug/inspect .desc file parsing stages'
long_desc <<~DESC
  Shows the intermediate parsing stages for a .desc file:
  tokens, AST, and IR. Useful for debugging grammar issues.

  Examples:
    descent debug parser.desc           # Show all stages
    descent debug parser.desc --tokens  # Show tokens only
    descent debug parser.desc --ast     # Show AST only
    descent debug parser.desc --ir      # Show IR only
DESC

required_arg :file, desc: '.desc specification file'
flag :tokens, '--tokens', desc: 'Show tokens only'
flag :ast, '--ast', desc: 'Show AST only'
flag :ir, '--ir', desc: 'Show IR only'

def run
  require 'descent'

  unless File.exist?(file)
    Output.error "File not found: #{file}"
    exit 1
  end

  # Default: show all if no specific flag
  show_all = !tokens && !ast && !ir

  begin
    content    = File.read(file)
    token_list = Descent::Lexer.new(content, source_file: file).tokenize

    if tokens || show_all
      Output.header 'TOKENS'
      token_list.each_with_index do |t, i|
        puts format('%3d: L%-3d %-12s id=%-20s rest=%s',
                    i, t.lineno, t.tag.inspect, t.id.inspect, t.rest.inspect)
      end
      puts
    end

    ast_result = Descent::Parser.new(token_list).parse

    if ast || show_all
      Output.header 'AST'
      puts "Parser: #{ast_result.name.inspect}"
      puts "Entry:  #{ast_result.entry_point.inspect}"
      puts
      puts 'Types:'
      ast_result.types.each { |t| puts "  #{t.name}: #{t.kind}" }
      puts
      puts 'Functions:'
      ast_result.functions.each do |f|
        puts "  #{f.name}#{":#{f.return_type}" if f.return_type} #{f.params.map { |p| ":#{p}" }.join(' ')}"
        f.states.each do |s|
          puts "    state[:#{s.name}]"
          s.cases.each do |c|
            chars_desc = c.chars.nil? ? 'default' : "c[#{c.chars}]"
            puts "      #{chars_desc}#{" .#{c.substate}" if c.substate}"
            c.commands.each { |cmd| puts "        #{cmd.type}: #{cmd.value.inspect}" }
          end
        end
      end
      puts
    end

    if ir || show_all
      ir_result = Descent::IRBuilder.new(ast_result).build

      Output.header 'IR'
      puts "Parser: #{ir_result.name.inspect}"
      puts "Entry:  #{ir_result.entry_point.inspect}"
      puts
      puts 'Types:'
      ir_result.types.each do |t|
        flags = []
        flags << 'emits_start' if t.emits_start
        flags << 'emits_end' if t.emits_end
        puts "  #{t.name}: #{t.kind} #{flags.join(', ')}"
      end
      puts
      puts 'Functions:'
      ir_result.functions.each do |f|
        puts "  #{f.name}#{":#{f.return_type}" if f.return_type} #{f.params.map { |p| ":#{p}" }.join(' ')}"
        puts "    emits_events: #{f.emits_events}"
        puts "    expects_char: #{f.expects_char.inspect}" if f.expects_char
        puts "    locals: #{f.locals.inspect}" unless f.locals.empty?
        f.states.each do |s|
          scan_info = s.scannable? ? " SCAN(#{s.scan_chars.map(&:inspect).join(', ')})" : ''
          puts "    state[:#{s.name}]#{scan_info}"
          s.cases.each do |c|
            chars_desc = if c.conditional?
                           "if[#{c.condition}]"
                         elsif c.default?
                           'default'
                         elsif c.special_class
                           c.special_class.to_s.upcase
                         elsif c.chars
                           "c[#{c.chars.map { |ch| ch == "\n" ? '\\n' : ch }.join}]"
                         else
                           'c[???]'
                         end
            puts "      #{chars_desc}#{" .#{c.substate}" if c.substate}"
            c.commands.each { |cmd| puts "        #{cmd.type}: #{cmd.args.inspect}" }
          end
        end
      end
    end
  rescue Descent::Error => e
    Output.error e.message
    exit 1
  end
end
