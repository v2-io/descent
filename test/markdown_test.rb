# frozen_string_literal: true

require 'test_helper'

class MarkdownTest < Minitest::Test
  MARKDOWN_DESC = File.expand_path('../examples/markdown.desc', __dir__)

  def setup
    @content = File.read(MARKDOWN_DESC)
    @tokens  = Descent::Lexer.new(@content, source_file: MARKDOWN_DESC).tokenize
    @ast     = Descent::Parser.new(@tokens).parse
    @ir      = Descent::IRBuilder.new(@ast).build
  end

  # ============================================================
  # Basic parsing tests
  # ============================================================

  def test_parses_without_error = assert @ir, 'IR should be built successfully'

  def test_parser_name = assert_equal 'markdown', @ir.name

  def test_entry_point = assert_equal '/document', @ir.entry_point

  # ============================================================
  # Type declarations
  # ============================================================

  def test_has_bracket_types
    bracket_types = @ir.types.select { |t| t.kind == :bracket }.map(&:name)

    assert_includes bracket_types, 'Heading'
    assert_includes bracket_types, 'Paragraph'
    assert_includes bracket_types, 'CodeBlock'
    assert_includes bracket_types, 'Blockquote'
    assert_includes bracket_types, 'ListItem'
    assert_includes bracket_types, 'ThematicBreak'
    assert_includes bracket_types, 'Emphasis'
    assert_includes bracket_types, 'Strong'
    assert_includes bracket_types, 'Strikethrough'
  end

  def test_has_content_types
    content_types = @ir.types.select { |t| t.kind == :content }.map(&:name)

    assert_includes content_types, 'Text'
    assert_includes content_types, 'Code'
  end

  def test_bracket_types_emit_start_and_end
    @ir.types.select { |t| t.kind == :bracket }.each do |type|
      assert type.emits_start, "#{type.name} should emit start"
      assert type.emits_end, "#{type.name} should emit end"
    end
  end

  # ============================================================
  # Function declarations
  # ============================================================

  def test_has_expected_functions
    fn_names = @ir.functions.map(&:name)

    # Block-level
    assert_includes fn_names, 'document'
    assert_includes fn_names, 'heading'
    assert_includes fn_names, 'paragraph'
    assert_includes fn_names, 'code_block'
    assert_includes fn_names, 'blockquote'
    assert_includes fn_names, 'list_item'

    # Inline
    assert_includes fn_names, 'inline'
    assert_includes fn_names, 'emphasis'
    assert_includes fn_names, 'strong'
    assert_includes fn_names, 'code_span'
    assert_includes fn_names, 'text'
  end

  def test_emphasis_returns_emphasis_type
    emphasis_fn = @ir.functions.find { |f| f.name == 'emphasis' }
    assert_equal 'Emphasis', emphasis_fn.return_type
  end

  def test_strong_returns_strong_type
    strong_fn = @ir.functions.find { |f| f.name == 'strong' }
    assert_equal 'Strong', strong_fn.return_type
  end

  def test_strikethrough_returns_strikethrough_type
    strike_fn = @ir.functions.find { |f| f.name == 'strikethrough' }
    assert_equal 'Strikethrough', strike_fn.return_type
  end

  def test_underscore_emphasis_returns_emphasis_type
    emph_under_fn = @ir.functions.find { |f| f.name == 'emphasis_under' }
    assert_equal 'Emphasis', emph_under_fn.return_type
  end

  def test_underscore_strong_returns_strong_type
    strong_under_fn = @ir.functions.find { |f| f.name == 'strong_under' }
    assert_equal 'Strong', strong_under_fn.return_type
  end

  def test_list_item_has_col_parameter
    list_item_fn = @ir.functions.find { |f| f.name == 'list_item' }
    assert_includes list_item_fn.params, 'col'
  end

  # ============================================================
  # Emphasis flanking detection (PREV checks)
  # ============================================================

  def test_emphasis_has_prev_checks
    emphasis_fn      = @ir.functions.find { |f| f.name == 'emphasis' }
    check_star_state = emphasis_fn.states.find { |s| s.name == 'check_star' }

    # Should have conditional cases checking PREV
    conditionals = check_star_state.cases.select(&:conditional?)
    assert conditionals.any?, 'emphasis:check_star should have PREV conditionals'

    # Check for space, tab, newline, and 0 (start of input)
    conditions = conditionals.map(&:condition)
    assert conditions.any? { |c| c.include?("PREV == ' '") }, 'Should check PREV == space'
    assert conditions.any? { |c| c.include?('PREV == 0') }, 'Should check PREV == 0'
  end

  def test_strong_has_prev_checks
    strong_fn        = @ir.functions.find { |f| f.name == 'strong' }
    check_star_state = strong_fn.states.find { |s| s.name == 'check_star' }

    conditionals = check_star_state.cases.select(&:conditional?)
    assert conditionals.any?, 'strong:check_star should have PREV conditionals'
  end

  # ============================================================
  # Literal star emission
  # ============================================================

  def test_inline_emits_literal_stars
    inline_fn  = @ir.functions.find { |f| f.name == 'inline' }
    after_star = inline_fn.states.find { |s| s.name == 'after_star' }

    # Find the case that handles non-left-flanking *
    literal_case = after_star.cases.find do |c|
      c.commands.any? { |cmd| cmd.type == :inline_emit_literal }
    end

    assert literal_case, 'Should have inline_emit_literal for literal *'

    emit_cmd = literal_case.commands.find { |cmd| cmd.type == :inline_emit_literal }
    assert_equal 'Text', emit_cmd.args[:type]
    assert_equal '*', emit_cmd.args[:literal]
  end

  def test_inline_emits_literal_double_stars
    inline_fn = @ir.functions.find { |f| f.name == 'inline' }
    after_two = inline_fn.states.find { |s| s.name == 'after_two_stars' }

    literal_case = after_two.cases.find do |c|
      c.commands.any? { |cmd| cmd.type == :inline_emit_literal }
    end

    assert literal_case, 'Should have inline_emit_literal for literal **'

    emit_cmd = literal_case.commands.find { |cmd| cmd.type == :inline_emit_literal }
    assert_equal 'Text', emit_cmd.args[:type]
    assert_equal '**', emit_cmd.args[:literal]
  end

  # ============================================================
  # SCAN optimization inference
  # ============================================================

  def test_emph_text_has_scan_optimization
    # emph_text has 3 exit chars (`, *, \n) - fits memchr3
    emph_text_fn = @ir.functions.find { |f| f.name == 'emph_text' }
    main_state   = emph_text_fn.states.find { |s| s.name == 'main' }

    assert main_state.scannable?, 'emph_text:main should be scannable'
    assert_includes main_state.scan_chars, '`'
    assert_includes main_state.scan_chars, '*'
    assert_includes main_state.scan_chars, "\n"
  end

  def test_text_stops_at_inline_delimiters
    # text has 5 exit chars (`, *, _, ~, \n) - exceeds memchr3 limit
    # but still correctly identifies exit characters
    text_fn    = @ir.functions.find { |f| f.name == 'text' }
    main_state = text_fn.states.find { |s| s.name == 'main' }

    exit_chars = main_state.cases.reject(&:default?).map { |c| c.chars&.first }.compact
    assert_includes exit_chars, '`'
    assert_includes exit_chars, '*'
    assert_includes exit_chars, '_'
    assert_includes exit_chars, '~'
    assert_includes exit_chars, "\n"
  end

  def test_code_span_has_scan_optimization
    code_span_fn = @ir.functions.find { |f| f.name == 'code_span' }
    main_state   = code_span_fn.states.find { |s| s.name == 'main' }

    assert main_state.scannable?, 'code_span:main should be scannable'
    assert_includes main_state.scan_chars, '`'
  end

  # ============================================================
  # Rust code generation
  # ============================================================

  def test_generates_rust_without_error
    rust_code = Descent.generate(MARKDOWN_DESC, target: :rust)
    assert_kind_of String, rust_code
    assert_operator rust_code.length, :>, 1000, 'Generated code should be substantial'
  end

  def test_generated_rust_has_event_enum
    rust_code = Descent.generate(MARKDOWN_DESC, target: :rust)

    assert_match(/pub enum Event/, rust_code)
    assert_match(/EmphasisStart/, rust_code)
    assert_match(/EmphasisEnd/, rust_code)
    assert_match(/StrongStart/, rust_code)
    assert_match(/StrongEnd/, rust_code)
    assert_match(/StrikethroughStart/, rust_code)
    assert_match(/StrikethroughEnd/, rust_code)
  end

  def test_generated_rust_has_prev_method
    rust_code = Descent.generate(MARKDOWN_DESC, target: :rust)

    assert_match(/fn prev\(&self\) -> u8/, rust_code)
  end

  def test_generated_rust_has_prev_checks
    rust_code = Descent.generate(MARKDOWN_DESC, target: :rust)

    assert_match(/self\.prev\(\) == b' '/, rust_code)
    assert_match(/self\.prev\(\) == b'\\t'/, rust_code)
    assert_match(/self\.prev\(\) == b'\\n'/, rust_code)
    assert_match(/self\.prev\(\) == 0/, rust_code)
  end

  def test_generated_rust_emits_literal_stars
    rust_code = Descent.generate(MARKDOWN_DESC, target: :rust)

    assert_match(/Event::Text \{ content: b"\*"/, rust_code)
    assert_match(/Event::Text \{ content: b"\*\*"/, rust_code)
  end

  def test_generated_rust_has_recursive_emphasis
    rust_code = Descent.generate(MARKDOWN_DESC, target: :rust)

    # emphasis should call itself for nested emphasis
    assert_match(/parse_emphasis.*parse_emphasis/m, rust_code)
    # strong should call emphasis for nested
    assert_match(/parse_strong.*parse_emphasis/m, rust_code)
  end
end
