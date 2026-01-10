# frozen_string_literal: true

module Descent
  # Unified character class parser according to characters.md spec.
  #
  # Handles all character literal and class syntax:
  # - Single chars: 'x', '\n', '\x00'
  # - Strings: 'hello' (decomposed to chars for classes)
  # - Classes: <...> with space-separated tokens
  # - Predefined classes: LETTER, DIGIT, SQ, P, 0-9, etc.
  # - Empty class: <> (empty set / empty string)
  # - Param refs: :name
  #
  # The same parsing is used everywhere: c[...], function args, PREPEND
  module CharacterClass
    # Predefined single-character classes (DSL-reserved chars)
    SINGLE_CHAR = {
      'P' => '|',
      'L' => '[',
      'R' => ']',
      'LB' => '{',
      'RB' => '}',
      'LP' => '(',
      'RP' => ')',
      'SQ' => "'",
      'DQ' => '"',
      'BS' => '\\'
    }.freeze

    # Predefined character ranges
    RANGES = {
      '0-9' => '0123456789',
      '0-7' => '01234567',
      '0-1' => '01',
      'a-z' => 'abcdefghijklmnopqrstuvwxyz',
      'A-Z' => 'ABCDEFGHIJKLMNOPQRSTUVWXYZ',
      'a-f' => 'abcdef',
      'A-F' => 'ABCDEF'
    }.freeze

    # Predefined multi-character classes (expanded to char sets)
    MULTI_CHAR = {
      'LETTER' => RANGES['a-z'] + RANGES['A-Z'],
      'DIGIT' => RANGES['0-9'],
      'HEX_DIGIT' => RANGES['0-9'] + RANGES['a-f'] + RANGES['A-F'],
      'LABEL_CONT' => "#{RANGES['a-z']}#{RANGES['A-Z']}#{RANGES['0-9']}_-",
      'WS' => " \t",
      'NL' => "\n"
    }.freeze

    # Special classes that require runtime checks (can't be expanded to char list)
    SPECIAL_CLASSES = %w[XID_START XID_CONT XLBL_START XLBL_CONT].freeze

    class << self
      # Parse a class specification string and return structured result.
      #
      # @param str [String] The class specification (contents of c[...] or <...> or bare)
      # @param context [Symbol] :match (for c[...]), :bytes (for function args/PREPEND), :byte (single byte)
      # @return [Hash] { chars: [...], special_class: nil|Symbol, param_ref: nil|String, bytes: String }
      def parse(str, context: :match)
        return { chars: [], special_class: nil, param_ref: nil, bytes: '' } if str.nil? || str.empty?

        str = str.strip

        # Handle explicit class wrapper <...>
        if str.start_with?('<') && str.end_with?('>')
          inner = str[1...-1].strip
          return { chars: [], special_class: nil, param_ref: nil, bytes: '' } if inner.empty? # <>

          return parse_class_content(inner, context)
        end

        # Handle param reference :name
        if str.start_with?(':')
          param = str[1..]
          return { chars: [], special_class: nil, param_ref: param, bytes: nil }
        end

        # Handle quoted string 'content'
        if str.match?(/^'.*'$/) && str.length >= 2
          content = parse_quoted_string(str[1...-1])
          chars   = content.chars
          return { chars: chars, special_class: nil, param_ref: nil, bytes: content }
        end

        # Handle double-quoted string "content"
        if str.match?(/^".*"$/) && str.length >= 2
          content = str[1...-1]
          chars   = content.chars
          return { chars: chars, special_class: nil, param_ref: nil, bytes: content }
        end

        # Check if it's a bare shorthand (only /[A-Za-z0-9_-]/ allowed)
        if str.match?(/^[A-Za-z0-9_-]+$/)
          # Could be a predefined class name or bare chars
          upper = str.upcase
          if SPECIAL_CLASSES.include?(upper)
            return { chars: [], special_class: upper.downcase.to_sym, param_ref: nil, bytes: nil }
          elsif MULTI_CHAR.key?(upper)
            chars = MULTI_CHAR[upper].chars
            return { chars: chars, special_class: nil, param_ref: nil, bytes: MULTI_CHAR[upper] }
          elsif SINGLE_CHAR.key?(upper)
            char = SINGLE_CHAR[upper]
            return { chars: [char], special_class: nil, param_ref: nil, bytes: char }
          elsif RANGES.key?(str)
            chars = RANGES[str].chars
            return { chars: chars, special_class: nil, param_ref: nil, bytes: RANGES[str] }
          else
            # Bare alphanumeric - decompose to individual chars
            chars = str.chars
            return { chars: chars, special_class: nil, param_ref: nil, bytes: str }
          end
        end

        # If we get here, it's invalid bare content (special chars without quotes)
        # For now, treat as literal bytes but this should probably error
        { chars: str.chars, special_class: nil, param_ref: nil, bytes: str }
      end

      # Parse the content inside <...> (space-separated tokens)
      def parse_class_content(content, context)
        return { chars: [], special_class: nil, param_ref: nil, bytes: '' } if content.nil? || content.empty?

        all_chars     = []
        all_bytes     = +''
        special_class = nil
        param_ref     = nil

        tokens = tokenize_class_content(content)

        tokens.each do |token|
          result = parse(token, context: context)

          if result[:special_class]
            # Only one special class allowed
            special_class = result[:special_class]
          elsif result[:param_ref]
            param_ref = result[:param_ref]
          else
            all_chars.concat(result[:chars]) if result[:chars]
            all_bytes << result[:bytes] if result[:bytes]
          end
        end

        { chars: all_chars.uniq, special_class: special_class, param_ref: param_ref, bytes: all_bytes }
      end

      # Tokenize class content respecting quotes
      def tokenize_class_content(content)
        tokens   = []
        current  = +''
        in_quote = false
        i        = 0

        while i < content.length
          c = content[i]

          if c == "'" && !in_quote
            in_quote = true
            current << c
          elsif c == "'" && in_quote
            current << c
            in_quote = false
          elsif c == '\\' && in_quote && i + 1 < content.length
            current << c << content[i + 1]
            i += 1
          elsif c == ' ' && !in_quote
            tokens << current unless current.empty?
            current = +''
          else
            current << c
          end

          i += 1
        end

        tokens << current unless current.empty?
        tokens
      end

      # Parse a quoted string with escape sequences
      def parse_quoted_string(str)
        result = +''
        i      = 0

        while i < str.length
          if str[i] == '\\'
            if i + 1 < str.length
              case str[i + 1]
              when 'n'  then result << "\n"
                             i += 2
              when 't'  then result << "\t"
                             i += 2
              when 'r'  then result << "\r"
                             i += 2
              when '\\' then result << '\\'
                             i += 2
              when "'"  then result << "'"
                             i  += 2
              when '"'  then result << '"'
                             i  += 2
              when 'x'
                # Hex byte: \xHH
                if i + 3 < str.length && str[i + 2..i + 3].match?(/^[0-9A-Fa-f]{2}$/)
                  result << str[i + 2..i + 3].to_i(16).chr
                  i += 4
                else
                  result << str[i + 1]
                  i += 2
                end
              when 'u'
                # Unicode: \uXXXX
                if i + 5 < str.length && str[i + 2..i + 5].match?(/^[0-9A-Fa-f]{4}$/)
                  result << str[i + 2..i + 5].to_i(16).chr(Encoding::UTF_8)
                  i += 6
                else
                  result << str[i + 1]
                  i += 2
                end
              when '0'
                # Null byte
                result << "\0"
                i += 2
              else
                result << str[i + 1]
                i += 2
              end
            else
              result << str[i]
              i += 1
            end
          else
            result << str[i]
            i += 1
          end
        end

        result
      end

      # Convert parsed result to Rust byte literal format for :byte param (u8)
      def to_rust_byte(result)
        return result[:param_ref] if result[:param_ref]
        return '0u8' if result[:chars].empty? && result[:bytes].empty? # Empty = never match sentinel

        char = result[:bytes][0] || result[:chars][0]
        escape_rust_byte(char)
      end

      # Convert parsed result to Rust byte string format for :bytes param (&[u8])
      def to_rust_bytes(result)
        return result[:param_ref] if result[:param_ref]
        return 'b""' if result[:bytes].nil? || result[:bytes].empty?

        "b\"#{escape_rust_byte_string(result[:bytes])}\""
      end

      # Escape a single character for Rust byte literal b'x'
      def escape_rust_byte(char)
        escaped = case char
                  when "\n" then '\\n'
                  when "\t" then '\\t'
                  when "\r" then '\\r'
                  when "\0" then '\\0'
                  when '\\' then '\\\\'
                  when "'" then "\\'"
                  else
                    if char.ord < 32 || char.ord > 126
                      format('\\x%02x', char.ord)
                    else
                      char
                    end
                  end
        "b'#{escaped}'"
      end

      # Escape a string for Rust byte string literal b"..."
      def escape_rust_byte_string(str)
        str.chars.map do |char|
          case char
          when "\n" then '\\n'
          when "\t" then '\\t'
          when "\r" then '\\r'
          when "\0" then '\\0'
          when '\\' then '\\\\'
          when '"' then '\\"'
          else
            if char.ord < 32 || char.ord > 126
              format('\\x%02x', char.ord)
            else
              char
            end
          end
        end.join
      end
    end
  end

  # Transforms AST into IR with semantic analysis.
  #
  # Responsibilities:
  # - Resolve type references
  # - Infer SCAN optimization characters from state structure
  # - Infer EOF handling requirements
  # - Collect local variable declarations
  # - Validate consistency
  class IRBuilder
    def initialize(ast) = @ast = ast

    def build
      types     = build_types(@ast.types)
      functions = @ast.functions.map { |f| build_function(f, types) }
      keywords  = @ast.keywords.map { |k| build_keywords(k) }

      # Collect custom error codes from /error(code) calls
      custom_error_codes = collect_custom_error_codes(functions)

      # Collect prepend values by tracing call sites
      functions = collect_prepend_values(functions)

      # Transform call arguments based on target parameter types
      functions = transform_call_args_by_type(functions)

      IR::Parser.new(
        name: @ast.name,
        entry_point: @ast.entry_point,
        types:,
        functions:,
        keywords:,
        custom_error_codes:
      )
    end

    private

    # Transform AST Keywords to IR Keywords
    def build_keywords(kw)
      # Parse the fallback function call (e.g., "/bare_string" or "/bare_string(arg)")
      fallback_func = nil
      fallback_args = nil

      if kw.fallback
        if kw.fallback =~ %r{^/(\w+)\(([^)]*)\)$}
          fallback_func = ::Regexp.last_match(1)
          fallback_args = ::Regexp.last_match(2)
        elsif kw.fallback =~ %r{^/(\w+)$}
          fallback_func = ::Regexp.last_match(1)
        end
      end

      IR::Keywords.new(
        name: kw.name,
        fallback_func:,
        fallback_args:,
        mappings: kw.mappings,
        lineno: kw.lineno
      )
    end

    def build_types(type_decls)
      type_decls.map do |t|
        emits_start = t.kind == :BRACKET
        emits_end   = t.kind == :BRACKET

        IR::TypeInfo.new(
          name: t.name,
          kind: t.kind.downcase.to_sym,
          emits_start:,
          emits_end:,
          lineno: t.lineno
        )
      end
    end

    def build_function(func, types)
      return_type_info = types.find { |t| t.name == func.return_type }
      emits_events     = return_type_info&.bracket? || return_type_info&.content?

      locals = infer_locals(func)
      states = func.states.map { |s| build_state(s, func.params) }

      # Infer expected closing delimiter from return cases
      expects_char, emits_content_on_close = infer_expects(states)

      # Infer parameter types from usage (byte if used in |c[:x]|, i32 otherwise)
      param_types = infer_param_types(func.params, states)

      # Transform function-level eof_handler commands from AST to IR
      # Apply the same inline emit fix as for case commands
      func_eof_commands = func.eof_handler&.commands&.map { |c| build_command(c) }
      func_eof_commands = mark_returns_after_inline_emits(func_eof_commands) if func_eof_commands

      # Transform entry_actions (initialization commands on function entry)
      entry_actions = func.entry_actions&.map { |c| build_command(c) } || []

      IR::Function.new(
        name: func.name,
        return_type: func.return_type,
        params: func.params,
        param_types:,
        locals:,
        states:,
        eof_handler: func_eof_commands,
        entry_actions:,
        emits_events:,
        expects_char:,
        emits_content_on_close:,
        lineno: func.lineno
      )
    end

    def build_state(state, params = [])
      cases           = state.cases.map { |c| build_case(c, params) }
      scan_chars      = infer_scan_chars(state, cases)
      is_self_looping = cases.any? { |c| c.default? && has_self_transition?(c) }

      # Check if state has a default case (no chars, no condition, no special_class, no param_ref)
      has_default = cases.any?(&:default?)

      # Check if first case is unconditional (bare action - no char match)
      # This means the state just executes actions without matching any character
      # Note: param_ref IS a match (against a param value), so it's not unconditional
      first_case = cases.first
      is_unconditional = first_case && first_case.chars.nil? && first_case.special_class.nil? &&
                         first_case.param_ref.nil? && first_case.condition.nil?

      # Transform eof_handler commands from AST to IR
      # Apply the same inline emit fix as for case commands
      eof_commands = state.eof_handler&.commands&.map { |c| build_command(c) }
      eof_commands = mark_returns_after_inline_emits(eof_commands) if eof_commands

      # Inject '\n' into scan_chars if not already a user target (and room available).
      # This ensures SIMD scans stop at newlines for correct line/column tracking.
      # The template adds a match arm for '\n' that updates line/col and continues scanning.
      newline_injected = false
      if scan_chars && !scan_chars.include?("\n") && scan_chars.size < 6
        scan_chars       = ["\n"] + scan_chars # Prepend so newline is checked first
        newline_injected = true
      end

      IR::State.new(
        name: state.name,
        cases:,
        eof_handler: eof_commands,
        scan_chars:,
        is_self_looping:,
        has_default:,
        is_unconditional:,
        newline_injected:,
        lineno: state.lineno
      )
    end

    def build_case(kase, params = [])
      validate_char_syntax(kase.chars, kase.lineno) if kase.chars
      validate_prepend_commands(kase.commands, params, kase.lineno)
      validate_call_args(kase.commands, params, kase.lineno)
      chars, special_class, param_ref = parse_chars(kase.chars, params:)
      commands = kase.commands.map { |c| build_command(c) }

      # Fix #11: If inline emit precedes a bare return, mark return to suppress auto-emit
      # This prevents CONTENT functions from emitting twice (once for inline, once for auto)
      commands = mark_returns_after_inline_emits(commands)

      IR::Case.new(
        chars:,
        special_class:,
        param_ref:,
        condition: kase.condition,
        substate: kase.substate,
        commands:,
        lineno: kase.lineno
      )
    end

    # Mark return commands that follow inline emits to suppress auto-emit.
    # When a case has: | Float(USE_MARK) |return
    # The inline emit already emits, so return should NOT auto-emit.
    def mark_returns_after_inline_emits(commands)
      has_inline_emit = false

      commands.map do |cmd|
        case cmd.type
        when :inline_emit_bare, :inline_emit_mark, :inline_emit_literal
          has_inline_emit = true
          cmd
        when :return
          if has_inline_emit && cmd.args[:emit_type].nil? && cmd.args[:return_value].nil?
            # Bare return after inline emit - suppress auto-emit
            IR::Command.new(type: :return, args: cmd.args.merge(suppress_auto_emit: true))
          else
            cmd
          end
        else
          cmd
        end
      end
    end

    def build_command(cmd)
      # Handle AST::Conditional specially
      if cmd.is_a?(AST::Conditional)
        return IR::Command.new(
          type: :conditional,
          args: {
            clauses: cmd.clauses&.map do |c|
              {
                'condition' => c.condition,
                'commands' => c.commands.map { |cc| build_command(cc) }
              }
            end
          }
        )
      end

      args = case cmd.type
             when :assign, :add_assign, :sub_assign then cmd.value.is_a?(Hash) ? cmd.value : {}
             when :advance_to then { value: validate_advance_to(cmd.value, cmd.lineno) }
             when :scan then { value: process_escapes(cmd.value) }
             when :emit, :call_method, :transition, :error then { value: cmd.value }
             when :call then parse_call_value(cmd.value)
             when :inline_emit_bare, :inline_emit_mark then { type: cmd.value }
             when :inline_emit_literal then cmd.value.is_a?(Hash) ? cmd.value : {}
             when :term then { offset: cmd.value || 0 }
             when :prepend then { literal: process_escapes(cmd.value) }
             when :prepend_param then { param_ref: cmd.value }
             when :keywords_lookup then { name: cmd.value }
             when :return then parse_return_value(cmd.value)
             when :advance, :mark, :noop then {}
             else
               raise ValidationError, "Unknown command type: #{cmd.type.inspect}"
             end

      IR::Command.new(type: cmd.type, args:)
    end

    # Process character class/literal to get the actual bytes.
    # Uses unified CharacterClass parser.
    def process_escapes(str)
      return '' if str.nil? || str.empty?

      result = CharacterClass.parse(str)
      result[:bytes] || ''
    end

    # Validate and process advance_to (->[...]) arguments.
    # Only literal bytes are supported (1-6 chars for SIMD memchr).
    # Special classes and param refs are NOT supported.
    def validate_advance_to(str, lineno)
      raise ValidationError, "L#{lineno}: ->[] requires at least one character" if str.nil? || str.empty?

      result = CharacterClass.parse(str)

      if result[:special_class]
        raise ValidationError,
              "L#{lineno}: ->[] does not support character classes like #{str.upcase}. " \
              'Only literal bytes are supported (uses SIMD memchr).'
      end

      if result[:param_ref]
        raise ValidationError,
              "L#{lineno}: ->[] does not support parameter references like :#{result[:param_ref]}. " \
              'Only literal bytes are supported (uses SIMD memchr).'
      end

      bytes = result[:bytes] || ''
      raise ValidationError, "L#{lineno}: ->[] resolved to empty bytes from '#{str}'" if bytes.empty?

      if bytes.length > 6
        raise ValidationError,
              "L#{lineno}: ->[#{str}] has #{bytes.length} chars but maximum is 6 " \
              '(chained memchr limit). Split into multiple scans or restructure grammar.'
      end

      bytes
    end

    # Characters that MUST be quoted or use predefined class names in c[...]
    # These cause lexer/parser issues if used bare
    MUST_QUOTE_CHARS = {
      "'" => '<SQ>',      # Single quote - causes unterminated quote issues
      '|' => '<P>',       # Pipe - DSL delimiter
      '[' => '<L>',       # Open bracket - DSL delimiter
      ']' => '<R>',       # Close bracket - DSL delimiter
      ' ' => "' ' or <WS>" # Space - invisible, easy to miss
    }.freeze

    # Characters that SHOULD be quoted for clarity (warnings, not errors)
    SHOULD_QUOTE_CHARS = {
      '{' => '<LB>',
      '}' => '<RB>',
      '(' => '<LP>',
      ')' => '<RP>',
      '"' => '<DQ>',
      '\\' => '<BS>'
    }.freeze

    # Validate character syntax in c[...] before parsing.
    # Raises ValidationError for fatal issues.
    #
    # Valid syntax:
    #   - c[<...>]     - class syntax (space-separated tokens inside)
    #   - c['...']     - quoted string/char
    #   - c[:param]    - parameter reference
    #   - c[CLASS]     - predefined class (LETTER, DIGIT, etc.)
    #   - c[abc]       - bare alphanumeric/underscore/hyphen chars only
    #
    # Invalid:
    #   - c["]         - special chars must be quoted: c['"']
    #   - c[ ]         - spaces must be quoted: c[' ']
    #   - c[|]         - DSL chars must use escapes: c[<P>] or c['|']
    def validate_char_syntax(chars_str, lineno)
      return if chars_str.nil? || chars_str.empty?

      # Already using proper class syntax - <...> wrapper around everything
      return if chars_str.start_with?('<') && chars_str.end_with?('>')

      # Properly quoted string - validated by string parsing
      return if chars_str.start_with?("'") && chars_str.end_with?("'") && chars_str.length >= 2

      # Check for parameter reference (starts with : followed by valid identifier)
      return if chars_str.match?(/^:[a-z_]\w*$/i)

      # Check for pure special class (SCREAMING_CASE like LETTER, DIGIT, LABEL_CONT)
      return if chars_str.match?(/^[A-Z]+(?:_[A-Z]+)*$/)

      # Check for <TOKEN> escape sequences used OUTSIDE of a proper <...> class wrapper
      if chars_str.match?(/<[A-Z]+>/)
        raise ValidationError, "Line #{lineno}: Escape sequence like <SQ>, <P> etc. found outside " \
                               "class wrapper in c[#{chars_str}]. " \
                               'Wrap everything in a class: c[<...>] not c[THING <ESC> ...]'
      end

      # Check for combined class + chars (e.g., LETTER'[.?!)
      if chars_str.match?(/^[A-Z]+(?:_[A-Z]+)*'/)
        class_name = chars_str.match(/^([A-Z_]+)/)[1]
        raise ValidationError, "Line #{lineno}: Invalid character syntax in c[#{chars_str}]. " \
                               'Bare quote after class name is ambiguous. ' \
                               "Use class syntax instead: c[<#{class_name} ...>]"
      end

      # Check for unterminated quotes
      quote_count = chars_str.count("'")
      if quote_count.odd?
        raise ValidationError, "Line #{lineno}: Unterminated quote in c[#{chars_str}]. " \
                               'Single quotes must be paired. ' \
                               "To match a literal quote, use c[<SQ>] or c['\\'']"
      end

      # Check for any character outside /A-Za-z0-9_-/ that isn't quoted
      # These must be in single quotes or use escape sequences
      chars_str.each_char.with_index do |ch, i|
        next if ch.match?(/[A-Za-z0-9_-]/)
        next if ch == "'" # Quote chars are handled by quote pairing check
        next if ch == '\\' # Escape sequences handled separately

        # Check if this char is inside quotes
        quote_depth = chars_str[0...i].count("'")
        next if quote_depth.odd? # Inside quotes, OK

        # Special chars outside quotes - error
        suggestion = case ch
                     when '|' then "c[<P>] or c['|']"
                     when '[' then "c[<L>] or c['[']"
                     when ']' then "c[<R>] or c[']']"
                     when '{' then "c[<LB>] or c['{']"
                     when '}' then "c[<RB>] or c['}']"
                     when '(' then "c[<LP>] or c['(']"
                     when ')' then "c[<RP>] or c[')']"
                     when '"' then "c[<DQ>] or c['\"']"
                     when '\\' then "c[<BS>] or c['\\\\']"
                     when ' ' then "c[<WS>] or c[' ']"
                     when "\t" then "c['\\t']"
                     when "\n" then "c['\\n']"
                     else "c['#{ch}']"
                     end

        raise ValidationError, "Line #{lineno}: Unquoted '#{ch.inspect[1...-1]}' in c[#{chars_str}]. " \
                               "Characters outside /A-Za-z0-9_-/ must be quoted. Use #{suggestion}"
      end
    end

    # Validate PREPEND commands for common mistakes.
    # Catches: PREPEND(param) where param is a known parameter name - should be PREPEND(:param)
    def validate_prepend_commands(commands, params, lineno)
      return if params.empty?

      commands.each do |cmd|
        next unless cmd.type == :prepend
        next if cmd.value.nil?

        literal = cmd.value.to_s.strip

        # Check if the literal matches a param name (bare word without quotes)
        # Valid literals: 'x', '|', '``', <P>, etc.
        # Suspicious: prepend (bare word matching param name)
        next unless literal.match?(/^[a-z_]\w*$/i) # Bare identifier
        next unless params.include?(literal)

        raise ValidationError, "Line #{lineno}: PREPEND(#{literal}) looks like a parameter reference. " \
                               "Use PREPEND(:#{literal}) to reference the '#{literal}' parameter, " \
                               "or PREPEND('#{literal}') for a literal string."
      end
    end

    # Validate function call arguments for bare identifiers matching param names.
    # Catches: /func(param) where param is a known parameter - should be /func(:param)
    def validate_call_args(commands, params, lineno)
      return if params.empty?

      commands.each do |cmd|
        next unless cmd.type == :call
        next if cmd.value.nil?

        # cmd.value is like "text(prepend)" or "func" - extract args if present
        call_str = cmd.value.to_s
        next unless call_str.include?('(')

        args_str = call_str[/\((.+)\)/, 1]
        next if args_str.nil? || args_str.empty?

        # Tokenize respecting quotes and angle brackets
        args = tokenize_call_args_for_validation(args_str)

        args.each do |arg|
          arg = arg.strip
          # Skip if it's already a proper reference (:param), quoted, or class syntax
          next if arg.start_with?(':')      # :param - correct
          next if arg.start_with?("'")      # 'literal'
          next if arg.start_with?('"')      # "literal"
          next if arg.start_with?('<')      # <CLASS>
          next if arg.match?(/^-?\d+$/)     # numeric
          next if arg.match?(/^[A-Z]+$/)    # COL, LINE, PREV - built-in vars
          next if arg.include?(' ')         # expression like "COL - 1"
          next if arg.include?('.')         # method call
          next if arg.include?('(')         # function call

          # Bare lowercase identifier - check if it matches a param name
          next unless arg.match?(/^[a-z_]\w*$/i)
          next unless params.include?(arg)

          raise ValidationError, "Line #{lineno}: /...(...#{arg}...) - bare identifier '#{arg}' matches a parameter name. " \
                                 "Use ':#{arg}' to pass the parameter value, or \"'#{arg}'\" for a literal string."
        end
      end
    end

    # Tokenize call args for validation, respecting quotes and angle brackets
    def tokenize_call_args_for_validation(args_str)
      args     = []
      current  = +''
      in_quote = false
      in_angle = 0

      args_str.each_char do |c|
        case c
        when "'"
          in_quote = !in_quote
          current << c
        when '<'
          in_angle += 1
          current << c
        when '>'
          in_angle -= 1 if in_angle.positive?
          current << c
        when ','
          if in_quote || in_angle.positive?
            current << c
          else
            args << current.strip
            current = +''
          end
        else
          current << c
        end
      end

      args << current.strip unless current.empty?
      args
    end

    # Parse character specification into literal chars, special class, and/or param reference.
    # Returns [chars_array, special_class_symbol, param_ref_string]
    #
    # Supports both legacy syntax and new characters.md syntax:
    #
    # Legacy (backwards compatible):
    #   "abc"           -> [["a", "b", "c"], nil, nil]
    #   "LETTER"        -> [nil, :letter, nil]
    #   "LETTER'[.?!"   -> [["'", "[", ".", "?", "!"], :letter, nil]
    #   ":close"        -> [nil, nil, "close"]  (param reference)
    #
    # New syntax (characters.md):
    #   "'x'"           -> [["x"], nil, nil] (quoted char)
    #   "'abc'"         -> [["a", "b", "c"], nil, nil] (quoted string, decomposed)
    #   "<abc>"         -> [["a", "b", "c"], nil, nil] (class with bare lowercase)
    #   "<LETTER>"      -> [nil, :letter, nil] (class with predefined class)
    #   "<0-9>"         -> [["0".."9"], nil, nil] (predefined range)
    #   "<LETTER 0-9 '_'>" -> [["_", "0".."9"], :letter, nil] (combined)
    #   "<:var>"        -> [nil, nil, "var"] (variable in class)
    # Parse character specification for c[...] using unified CharacterClass parser.
    # Returns [chars_array, special_class_symbol, param_ref_string]
    def parse_chars(chars_str, params: [])
      return [nil, nil, nil] if chars_str.nil?

      # Use unified CharacterClass parser
      result = CharacterClass.parse(chars_str)

      # Validate param_ref against known params
      if result[:param_ref] && !params.include?(result[:param_ref])
        # Unknown param - treat the whole thing as literal chars
        chars = ":#{result[:param_ref]}".chars
        return [chars, nil, nil]
      end

      chars = result[:chars].empty? ? nil : result[:chars]
      [chars, result[:special_class], result[:param_ref]]
    end

    # Legacy: Parse escape sequences in a quoted string.
    # Kept for backwards compatibility but CharacterClass.parse_quoted_string is preferred.
    def parse_quoted_string(str)
      return '' if str.nil? || str.empty?

      result = +''
      i      = 0
      while i < str.length
        if str[i] == '\\' && i + 1 < str.length
          case str[i + 1]
          when 'n'  then result << "\n"
          when 't'  then result << "\t"
          when 'r'  then result << "\r"
          when '\\' then result << '\\'
          when "'"  then result << "'"
          when 'x'
            # Hex byte: \xHH
            if i + 3 < str.length && str[i + 2..i + 3].match?(/^[0-9A-Fa-f]{2}$/)
              result << str[i + 2..i + 3].to_i(16).chr
              i += 2
            else
              result << str[i + 1]
            end
          when 'u'
            # Unicode: \uXXXX
            if i + 5 < str.length && str[i + 2..i + 5].match?(/^[0-9A-Fa-f]{4}$/)
              result << str[i + 2..i + 5].to_i(16).chr(Encoding::UTF_8)
              i += 4
            else
              result << str[i + 1]
            end
          else
            result << str[i + 1]
          end
          i += 2
        else
          result << str[i]
          i += 1
        end
      end
      result
    end

    # Parse return value specification
    # Returns hash with :emit_type, :emit_mode, :literal, :return_value
    # Examples:
    #   nil or ""        -> {} (default behavior)
    #   "TypeName"       -> { emit_type: "TypeName", emit_mode: :bare }
    #   "TypeName(USE_MARK)" -> { emit_type: "TypeName", emit_mode: :mark }
    #   "TypeName(lit)"  -> { emit_type: "TypeName", emit_mode: :literal, literal: "lit" }
    #   "varname"        -> { return_value: "varname" } (for INTERNAL types returning a value)
    def parse_return_value(value)
      return {} if value.nil? || value.empty?

      case value
      when /^([A-Z]\w*)\(USE_MARK\)$/ then { emit_type: ::Regexp.last_match(1), emit_mode: :mark }
      when /^([A-Z]\w*)\(([^)]+)\)$/
        { emit_type: ::Regexp.last_match(1), emit_mode: :literal, literal: process_escapes(::Regexp.last_match(2)) }
      when /^([A-Z]\w*)$/ then { emit_type: ::Regexp.last_match(1), emit_mode: :bare }
      when /^[a-z_]\w*$/
        # Variable name - for INTERNAL types returning a computed value
        { return_value: value }
      else
        {} # Unknown format, use default
      end
    end

    # Parse a call command value into name and args.
    # Examples:
    #   "func"           -> { name: "func", call_args: nil }
    #   "func(x, y)"     -> { name: "func", call_args: "x, y" }
    #   "func(<R>)"      -> { name: "func", call_args: "<R>" }
    #   "func())"        -> { name: "func", call_args: ")" }  (bare paren as arg)
    #   "error(Code)"    -> { name: "error", call_args: "Code", is_error: true }
    def parse_call_value(value)
      return { name: value, call_args: nil } unless value.include?('(')

      # Find the first '(' - everything before is the name
      paren_pos = value.index('(')
      name      = value[0...paren_pos]

      # Everything after the first '(' up to the last ')' is the args
      # For "func())" -> args = ")"
      # For "func(<R>)" -> args = "<R>"
      rest = value[(paren_pos + 1)..]

      # Strip the final ')' if present - but only ONE trailing paren
      call_args = rest.end_with?(')') ? rest[0...-1] : rest
      call_args = nil if call_args.empty?

      result = { name:, call_args: }
      result[:is_error] = true if name == 'error'
      result
    end

    # Infer SCAN optimization: if a state has a simple self-looping default case
    # (only advance + transition, no side effects), the explicit character cases
    # become SCAN targets.
    def infer_scan_chars(_state, cases)
      default_case = cases.find(&:default?)
      return nil unless default_case
      return nil unless simple_self_loop?(default_case)

      # Collect all explicit characters from non-default cases
      explicit_chars = cases
                       .reject(&:default?)
                       .reject(&:conditional?) # Skip conditional cases
                       .flat_map { |c| c.chars || [] }
                       .uniq

      return nil if explicit_chars.empty?
      # Support up to 6 chars via chained memchr calls (memchr3 + memchr3)
      # Beyond 6, the overhead of chaining outweighs the benefit
      return nil if explicit_chars.size > 6

      explicit_chars
    end

    # Check if a case is a simple self-loop: only advance and/or transition (no calls, emits, etc.)
    # This is the stricter check for SCAN optimization.
    def simple_self_loop?(kase)
      has_self_transition = false

      kase.commands.each do |cmd|
        case cmd.type
        when :advance
          # OK - just advancing
        when :transition
          val = cmd.args[:value] || cmd.args['value']
          has_self_transition = true if val.nil? || val.empty?
        else
          # Any other command (call, emit, mark, term, etc.) means not a simple loop
          return false
        end
      end

      has_self_transition
    end

    # Check if a case has any self-transition (used for is_self_looping metadata)
    def has_self_transition?(kase)
      kase.commands.any? do |cmd|
        next false unless cmd.type == :transition

        val = cmd.args[:value] || cmd.args['value']
        val.nil? || val.empty?
      end
    end

    # Infer expected closing delimiter from return cases.
    # If ALL return cases match the same single character, that's the expected closer.
    # Also check if TERM appears before return (emits_content_on_close).
    def infer_expects(states)
      return_cases = []

      # Collect all cases that contain a return command
      states.each do |state|
        state.cases.each do |kase|
          return_cases << kase if kase.commands.any? { |cmd| cmd.type == :return }
        end
      end

      # No returns found - no expected closer
      return [nil, false] if return_cases.empty?

      # Check if all return cases match the same single character
      # (ignore conditional cases for now - they still match on a char)
      char_matches = return_cases.filter_map do |kase|
        # Must have exactly one character match (not default, not char class)
        next nil if kase.default?
        next nil if kase.special_class
        next nil if kase.chars.nil? || kase.chars.length != 1

        kase.chars.first
      end

      # If not all return cases have single-char matches, no expected closer
      return [nil, false] if char_matches.length != return_cases.length

      # If not all the same character, no expected closer
      return [nil, false] if char_matches.uniq.length != 1

      expects_char = char_matches.first

      # Check if any return case has TERM before return
      emits_content = return_cases.any? do |kase|
        kase.commands.any? { |cmd| cmd.type == :term }
      end

      [expects_char, emits_content]
    end

    # Collect custom error codes from /error(code) calls across all functions
    def collect_custom_error_codes(functions)
      codes = Set.new

      functions.each do |func|
        func.states.each do |state|
          state.cases.each do |kase|
            collect_error_codes_from_commands(kase.commands, codes)
          end
        end
      end

      codes.to_a.sort
    end

    def collect_error_codes_from_commands(commands, codes)
      commands.each do |cmd|
        case cmd.type
        when :error
          # Explicit :error command
          code = cmd.args[:value] || cmd.args['value']
          codes << code if code && !code.empty?
        when :call
          # /error(code) is parsed as :call with is_error: true
          if cmd.args[:is_error]
            code = cmd.args[:call_args]
            codes << code if code && !code.empty?
          end
        when :conditional
          # Recurse into conditional clauses
          cmd.args[:clauses]&.each do |clause|
            collect_error_codes_from_commands(clause['commands'] || [], codes)
          end
        end
      end
    end

    # Infer local variables from assignments in function
    def infer_locals(func)
      locals = {}

      # Check entry_actions for variable declarations
      func.entry_actions&.each do |cmd|
        collect_locals_from_commands([cmd], locals)
      end

      # Check state cases for variable usage
      func.states.each do |state|
        state.cases.each do |kase|
          collect_locals_from_commands(kase.commands, locals)
        end
      end

      locals
    end

    def collect_locals_from_commands(commands, locals)
      commands.each do |cmd|
        if cmd.is_a?(AST::Conditional)
          cmd.clauses&.each do |clause|
            collect_locals_from_commands(clause.commands, locals)
          end
        elsif cmd.respond_to?(:type)
          case cmd.type
          when :assign, :add_assign, :sub_assign
            if cmd.value.is_a?(Hash) && cmd.value[:var]
              locals[cmd.value[:var]] ||= :i32 # Default type
            end
          end
        end
      end
    end

    # Infer parameter types from usage in states.
    # - Params used in |c[:x]| are bytes (u8) for single-byte comparison
    # - Params used in PREPEND(:x) are byte slices (&'static [u8]) for prepending
    # - Others default to i32
    def infer_param_types(params, states)
      return {} if params.empty?

      # Start with all params as i32 (default)
      types = params.to_h { |p| [p, :i32] }

      # Find params used in character matches (these become u8)
      # and params used in PREPEND (these become bytes slice)
      states.each do |state|
        state.cases.each do |kase|
          # Check param_ref in character matches - needs u8 for comparison
          types[kase.param_ref] = :byte if kase.param_ref && types.key?(kase.param_ref)

          # Check conditions for param == 'char' comparisons
          # e.g., |if[prepend == '|'] means prepend should be u8
          # Note: param == 0 is NOT a byte comparison - it's a numeric flag check
          if kase.condition
            params.each do |param|
              # Look for patterns like: param == 'x', 'x' == param (character literal comparisons)
              # Do NOT match param == 0 - that's a numeric comparison, not a byte sentinel
              next unless (kase.condition.match?(/\b#{Regexp.escape(param)}\s*[!=]=\s*'/) ||
                 kase.condition.match?(/'\s*[!=]=\s*#{Regexp.escape(param)}\b/)) && types.key?(param)

              types[param] = :byte
            end
          end

          # Check param_ref in PREPEND commands - needs &'static [u8] for prepending
          kase.commands.each do |cmd|
            if cmd.type == :prepend_param && cmd.args[:param_ref]
              param = cmd.args[:param_ref]
              types[param] = :bytes if types.key?(param)
            end
          end
        end
      end

      types
    end

    # Infer param types from call-site values AND propagate from callees.
    # If a function is called with bytes-like values (<>, <P>, '|'), that param becomes :bytes.
    # If bar calls foo(:x) and foo's param is :bytes, then bar's :x should be :bytes.
    def propagate_param_types(functions)
      func_by_name = functions.to_h { |f| [f.name, f] }

      # First pass: infer types from literal values at call sites
      functions.each do |func|
        func.states.each do |state|
          state.cases.each do |kase|
            kase.commands.each do |cmd|
              next unless cmd.type == :call && cmd.args[:call_args]

              target = func_by_name[cmd.args[:name]]
              next unless target

              args = cmd.args[:call_args].split(',').map(&:strip)
              args.zip(target.params).each do |arg, target_param|
                next unless target_param

                # If arg looks like a bytes value, mark target param as :bytes
                # BUT only if it's currently :i32 (default). Don't override :byte
                # which means it's used in |c[:x]| for single-byte comparison.
                if bytes_like_value?(arg) && target.param_types[target_param] == :i32
                  target.param_types[target_param] =
                    :bytes
                end
              end
            end
          end
        end
      end

      # Second pass: propagate types from callees to callers (iterative)
      changed = true
      while changed
        changed = false
        functions.each do |func|
          func.states.each do |state|
            state.cases.each do |kase|
              kase.commands.each do |cmd|
                next unless cmd.type == :call && cmd.args[:call_args]

                target = func_by_name[cmd.args[:name]]
                next unless target

                args = cmd.args[:call_args].split(',').map(&:strip)
                args.zip(target.params).each do |arg, target_param|
                  next unless target_param

                  # If arg is a param reference (:x), propagate type from callee
                  next unless arg.match?(/^:(\w+)$/)

                  our_param = arg[1..]
                  next unless func.param_types.key?(our_param)

                  target_type = target.param_types[target_param]
                  our_type = func.param_types[our_param]

                  # Propagate :bytes from callee to caller
                  if target_type == :bytes && our_type != :bytes
                    func.param_types[our_param] = :bytes
                    changed = true
                  # Propagate :byte from callee to caller (only if we're still default :i32)
                  elsif target_type == :byte && our_type == :i32
                    func.param_types[our_param] = :byte
                    changed = true
                  end
                end
              end
            end
          end
        end
      end

      functions
    end

    # Check if a value looks like a bytes literal.
    # These are DSL escape sequences and quoted strings that are clearly
    # meant to be byte content, not numeric values.
    # Note: Numeric values like 0 or -1 are NOT bytes-like - they're sentinels.
    # PREPEND params get typed as :bytes from infer_param_types (PREPEND usage),
    # not from call-site inference.
    # Check if a value MUST be a byte slice (not a single byte).
    # Only empty class <> definitively requires :bytes type.
    # Single-char values like '<P>' or '|' could be either :byte or :bytes,
    # so their type should be inferred from usage, not from call-site values.
    def bytes_like_value?(arg) = arg == '<>'

    # Collect prepend values by tracing call sites to functions with PREPEND(:param).
    # Returns updated functions with prepend_values filled in.
    def collect_prepend_values(functions)
      # First propagate param types from callees to callers
      functions = propagate_param_types(functions)

      func_by_name = functions.to_h { |f| [f.name, f] }

      # Step 1: Find which functions have PREPEND(:param) and which param it uses
      prepend_params = {} # func_name -> param_name
      functions.each do |func|
        func.states.each do |state|
          state.cases.each do |kase|
            kase.commands.each do |cmd|
              prepend_params[func.name] = cmd.args[:param_ref] if cmd.type == :prepend_param
            end
          end
        end
      end

      return functions if prepend_params.empty?

      # Step 2: Find all call sites and collect byte values passed
      prepend_values = Hash.new { |h, k| h[k] = Set.new }

      functions.each do |func|
        collect_call_values_from_states(func.states, prepend_params, func_by_name, prepend_values)
      end

      # Step 3: Update functions with prepend_values
      functions.map do |func|
        if prepend_params.key?(func.name)
          param_name = prepend_params[func.name]
          values = prepend_values[func.name].to_a.sort

          # Create updated function with prepend_values
          IR::Function.new(
            name: func.name,
            return_type: func.return_type,
            params: func.params,
            param_types: func.param_types,
            locals: func.locals,
            states: func.states,
            eof_handler: func.eof_handler,
            entry_actions: func.entry_actions,
            emits_events: func.emits_events,
            expects_char: func.expects_char,
            emits_content_on_close: func.emits_content_on_close,
            prepend_values: { param_name => values },
            lineno: func.lineno
          )
        else
          func
        end
      end
    end

    def collect_call_values_from_states(states, prepend_params, func_by_name, prepend_values)
      states.each do |state|
        state.cases.each do |kase|
          collect_call_values_from_commands(kase.commands, prepend_params, func_by_name, prepend_values)
        end
        collect_call_values_from_commands(state.eof_handler || [], prepend_params, func_by_name, prepend_values)
      end
    end

    def collect_call_values_from_commands(commands, prepend_params, func_by_name, prepend_values)
      commands.each do |cmd|
        case cmd.type
        when :call
          func_name = cmd.args[:name]
          next unless prepend_params.key?(func_name)

          # Extract the byte value from call_args
          call_args = cmd.args[:call_args]
          byte_value = parse_byte_literal(call_args)
          prepend_values[func_name] << byte_value if byte_value
        when :conditional
          cmd.args[:clauses]&.each do |clause|
            nested_cmds = (clause['commands'] || []).map { |c| c.is_a?(Hash) ? IR::Command.new(type: c['type'].to_sym, args: c['args'].transform_keys(&:to_sym)) : c }
            collect_call_values_from_commands(nested_cmds, prepend_params, func_by_name, prepend_values)
          end
        end
      end
    end

    # Transform call arguments based on target function parameter types.
    # For :bytes params, generates b"..." format; for :byte params, b'.' format.
    def transform_call_args_by_type(functions)
      func_by_name = functions.to_h { |f| [f.name, f] }

      functions.map do |func|
        new_states = func.states.map do |state|
          new_cases = state.cases.map do |kase|
            new_commands = transform_commands_args(kase.commands, func_by_name)
            IR::Case.new(
              chars: kase.chars,
              special_class: kase.special_class,
              param_ref: kase.param_ref,
              condition: kase.condition,
              substate: kase.substate,
              commands: new_commands,
              lineno: kase.lineno
            )
          end

          new_eof = transform_commands_args(state.eof_handler || [], func_by_name)

          IR::State.new(
            name: state.name,
            cases: new_cases,
            eof_handler: new_eof.empty? ? nil : new_eof,
            scan_chars: state.scan_chars,
            is_self_looping: state.is_self_looping,
            has_default: state.has_default,
            is_unconditional: state.is_unconditional,
            newline_injected: state.newline_injected,
            lineno: state.lineno
          )
        end

        IR::Function.new(
          name: func.name,
          return_type: func.return_type,
          params: func.params,
          param_types: func.param_types,
          locals: func.locals,
          states: new_states,
          eof_handler: func.eof_handler,
          entry_actions: func.entry_actions,
          emits_events: func.emits_events,
          expects_char: func.expects_char,
          emits_content_on_close: func.emits_content_on_close,
          prepend_values: func.prepend_values,
          lineno: func.lineno
        )
      end
    end

    def transform_commands_args(commands, func_by_name)
      commands.map do |cmd|
        if cmd.type == :call && cmd.args[:call_args]
          target_func = func_by_name[cmd.args[:name]]
          if target_func
            transformed_args = transform_args_for_target(cmd.args[:call_args], target_func)
            IR::Command.new(type: cmd.type, args: cmd.args.merge(call_args: transformed_args))
          else
            cmd
          end
        elsif cmd.type == :conditional
          new_clauses = cmd.args[:clauses]&.map do |clause|
            nested = (clause['commands'] || []).map do |c|
              c.is_a?(Hash) ? IR::Command.new(type: c['type'].to_sym, args: c['args'].transform_keys(&:to_sym)) : c
            end
            { 'condition' => clause['condition'], 'commands' => transform_commands_args(nested, func_by_name) }
          end
          IR::Command.new(type: cmd.type, args: { clauses: new_clauses })
        else
          cmd
        end
      end
    end

    # Transform call arguments based on target function's parameter types.
    # Uses CharacterClass for unified parsing, then converts to appropriate Rust format.
    def transform_args_for_target(args_str, target_func)
      return args_str if args_str.nil? || target_func.params.empty?

      args        = tokenize_call_args(args_str)
      params      = target_func.params
      param_types = target_func.param_types

      args.zip(params).map do |arg, param|
        next arg unless param

        param_type = param_types[param]

        # Handle numeric literals specially - they're numbers, not characters
        if arg.match?(/^-?\d+$/)
          case param_type
          when :bytes then 'b""'      # Numeric sentinel → empty bytes
          when :byte  then "#{arg}u8" # Numeric literal → u8
          else arg                    # :i32 → pass through
          end
        else
          case param_type
          when :bytes
            result = CharacterClass.parse(arg)
            CharacterClass.to_rust_bytes(result)
          when :byte
            result = CharacterClass.parse(arg)
            CharacterClass.to_rust_byte(result)
          else
            arg # :i32 or unknown, pass through
          end
        end
      end.join(', ')
    end

    # Tokenize call arguments respecting quotes (commas inside quotes don't split)
    def tokenize_call_args(args_str)
      args     = []
      current  = +''
      in_quote = false
      in_angle = 0

      args_str.each_char do |c|
        case c
        when "'"
          in_quote = !in_quote
          current << c
        when '<'
          in_angle += 1
          current << c
        when '>'
          in_angle -= 1 if in_angle.positive?
          current << c
        when ','
          if in_quote || in_angle.positive?
            current << c
          else
            args << current.strip
            current = +''
          end
        else
          current << c
        end
      end

      args << current.strip unless current.empty?
      args
    end

    # Parse a call argument into a byte literal string for the template.
    # Supports both legacy syntax and new characters.md syntax.
    def parse_byte_literal(arg)
      return nil if arg.nil? || arg.empty?

      case arg
      when '0' then nil # 0 means no prepend
      # Legacy escape syntax
      when '<P>' then '|'
      when '<L>' then '['
      when '<R>' then ']'
      when '<LB>' then '{'
      when '<RB>' then '}'
      when '<LP>' then '('
      when '<RP>' then ')'
      when '<BS>' then '\\\\'
      # New syntax: quoted single character
      when /^'(.)'$/ then ::Regexp.last_match(1)
      when /^"(.)"$/ then ::Regexp.last_match(1)
      # New syntax: quoted with escape (e.g., '\'')
      when /^'\\(.)'$/ then parse_quoted_string("\\#{::Regexp.last_match(1)}")
      # Legacy: single char
      when /^.$/ then arg
      else nil # Unknown format
      end
    end
  end
end
