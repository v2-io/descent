# Plan: Ruby Target for descent Parser Generator

## Summary

Add Ruby as a target language for descent, enabling generation of Ruby parsers from `.desc` specifications. The primary use case is generating a Ruby UDON parser from `libudon/generator/*.desc`.

## Design Decisions

1. **API**: Callback block pattern - `parser.parse { |event| ... }`
2. **Events**: Ruby `Data.define` objects (immutable, Ruby 3.2+)
3. **Optimization**: String#index for fast scanning (C-implemented)
4. **Binary handling**: Force binary encoding with `.b`, use `getbyte()`

## Files to Create

### 1. `lib/descent/templates/ruby/parser.liquid` (~600 lines)

Main template generating:
- Module wrapper with parser name
- `Span = Data.define(:start, :finish)`
- `Event` module with Data classes per type
- `ErrorCode` module with constants
- Keyword maps (Ruby Hash, not phf)
- `Parser` class with:
  - `initialize(input)` - setup instance vars
  - `parse(&block)` - entry point calling parse_{entry_point}
  - Helper methods: `peek`, `advance`, `mark`, `set_term`, `term`, `prepend_bytes`
  - `span`, `span_from_mark`, `col`, `prev`
  - Character class methods: `letter?`, `digit?`, `label_cont?`, `hex_digit?`, `ws?`, `nl?`
  - Unicode methods (if needed): `xid_start?`, `xid_cont?`, `xlbl_start?`, `xlbl_cont?`
  - SCAN methods: `scan_to1` through `scan_to6` using memchr gem
  - Keyword lookup methods: `lookup_{name}`, `lookup_{name}_or_fallback`
  - Parse functions: one per `.desc` function

### 2. `lib/descent/templates/ruby/_command.liquid` (~150 lines)

Command partial handling:
- `advance` -> `advance`
- `advance_to` -> `scan_to{n}(bytes...)`
- `mark` -> `mark`
- `term` -> `set_term(offset)`
- `prepend` -> `prepend_bytes("literal")`
- `prepend_param` -> `prepend_bytes(param)`
- `return` -> yield event + return
- `transition` -> `state = :name; next` or `next`
- `call` -> `parse_{name}(args, &on_event)`
- `assign/add_assign/sub_assign` -> Ruby assignment
- `emit` -> `yield Event::Type.new(...)`
- `inline_emit_bare/mark/literal` -> `yield Event::Type.new(...)`
- `error` -> `yield Event::Error.new(ErrorCode::CODE, span)`
- `keywords_lookup` -> `lookup_{name}_or_fallback(&on_event)`
- `conditional` -> `if/elsif/else/end`

## Files to Modify

### 3. `lib/descent/generator.rb`

Add Ruby-specific Liquid filters to `LiquidFilters` module:

```ruby
# Convert char to Ruby byte literal (Integer)
def escape_ruby_byte(char)
  # "\n" -> '10', "|" -> '124', etc.
end

# Escape string for Ruby string literal
def escape_ruby_string(str)
  # Handle \, ", \n, \t, \r
end

# Convert to snake_case
def snakecase(str)
  # "ElementStart" -> "element_start"
end

# Character class method name
def ruby_class_check(class_name)
  # "letter" -> "letter?"
end

# Transform DSL expressions to Ruby
def ruby_expr(str)
  # COL -> col, LINE -> @line, PREV -> prev
  # :param -> param
  # /function(args) -> parse_function(args, &on_event)
  # 'x' -> 120 (byte value)
  # <R> -> 93, <P> -> 124, etc.
end
```

## Key Implementation Details

### State Machine Translation

```ruby
# Single state -> simple loop
loop do
  case peek  # or case scan_to{n}(...) for scannable states
  when 10      # '\n'
    # commands...
  when ->(b) { letter?(b) }  # LETTER class
    # commands...
  else         # default
    # commands...
  end
end

# Multiple states -> symbol-based
state = :main
loop do
  case state
  when :main
    case peek
    # ...
    end
  when :content
    # ...
  end
end
```

### Event Types

```ruby
module Event
  # BRACKET types
  ElementStart = Data.define(:span)
  ElementEnd = Data.define(:span)

  # CONTENT types
  Text = Data.define(:content, :span)
  Name = Data.define(:content, :span)

  # Always present
  Error = Data.define(:code, :span)
end
```

### SCAN with String#index

Uses Ruby's C-implemented `String#index` for fast scanning. For multi-char
scans, calls index multiple times and takes minimum (10x faster than Regexp):

```ruby
def scan_to2(b1, b2)
  haystack = @input[@pos..]
  p1 = haystack.index(b1.chr(Encoding::BINARY))
  p2 = haystack.index(b2.chr(Encoding::BINARY))
  offset = [p1, p2].compact.min
  if offset
    update_line_col(haystack, offset)
    @pos += offset
    @input.getbyte(@pos)
  else
    update_line_col(haystack, haystack.bytesize)
    @pos = @input.bytesize
    nil
  end
end
```

### Unicode Classes

For XLBL_START/XLBL_CONT, we need Unicode XID support. Options:
1. Use `unicode-categories` gem (if available)
2. Simple ASCII fallback for spike (A-Za-z + limited Unicode check)

Recommended for spike: ASCII-only with UTF-8 continuation byte detection:
```ruby
def xlbl_start?(b)
  return false if b.nil?
  (b >= 65 && b <= 90) || (b >= 97 && b <= 122) || b >= 0xC2
end

def xlbl_cont?(b)
  xlbl_start?(b) || (b >= 48 && b <= 57) || b == 95 || b == 45 || b >= 0x80
end
```

## Trace Output

Trace output is essential for debugging generated parsers. The Ruby template will support `--trace` flag.

When `trace: true` in context, emit `warn` calls at key points:

```ruby
# On function entry
warn "TRACE: L#{lineno} ENTER #{func_name} | byte=#{trace_byte(peek)} pos=#{@pos}"

# On case match
warn "TRACE: L#{lineno} #{func_name}:#{state}.#{substate} | byte=#{trace_byte(peek)} term=#{trace_content} pos=#{@pos}"

# On EOF
warn "TRACE: L#{lineno} #{func_name}:#{state} EOF | term=#{trace_content} pos=#{@pos}"
```

Helper methods for trace (conditional on `trace` flag):

```ruby
def trace_byte(b)
  case b
  when nil then 'EOF'
  when 10 then "'\\n'"
  when 9 then "'\\t'"
  when 32 then "' '"
  when 33..126 then "'#{b.chr}'"
  else format('0x%02x', b)
  end
end

def trace_content
  term_end = @term_pos || @pos
  slice = @input[@mark_pos...term_end]
  prepend_info = @prepend_buf.empty? ? '' : "+#{@prepend_buf.bytesize}"
  return '[]' if slice.empty? && @prepend_buf.empty?
  s = slice.force_encoding(Encoding::UTF_8) rescue '<binary>'
  s.length > 32 ? "[#{s[0, 32].inspect}...]#{prepend_info}" : "[#{s.inspect}]#{prepend_info}"
end
```

## Streaming Support

Streaming is a primary reason for callback-style API. The Ruby template will include a `StreamingParser` class.

### Design

Line-oriented streaming: buffer input until newline, then parse complete lines. This matches UDON's line-oriented structure.

```ruby
# StreamEvent - events with owned content for cross-chunk safety
module StreamEvent
  ElementStart = Data.define(:span)
  ElementEnd = Data.define(:span)
  Text = Data.define(:content, :span)  # content is String, not slice
  Error = Data.define(:code, :span)
end

class StreamingParser
  def initialize(max_buffer: 4096)
    @buffer = +''
    @max_buffer = max_buffer
    @global_offset = 0
    @line = 1
    @column = 1
  end

  # Parse a chunk, yield events for complete lines
  # Returns :need_more_data or :complete
  def parse(chunk, &on_event)
    @buffer << chunk.b

    return :need_more_data if @buffer.empty?

    if @buffer.bytesize > @max_buffer
      yield StreamEvent::Error.new(:buffer_overflow, @global_offset..@global_offset)
      @buffer.clear
      return :complete
    end

    # Find last complete line
    last_newline = @buffer.rindex("\n")
    return :need_more_data unless last_newline

    # Extract and parse complete lines
    to_parse = @buffer.slice!(0, last_newline + 1)
    offset = @global_offset

    parser = Parser.new(to_parse)
    parser.instance_variable_set(:@line, @line)
    parser.instance_variable_set(:@column, @column)

    parser.parse do |event|
      yield to_stream_event(event, offset)
    end

    # Update state
    @global_offset += to_parse.bytesize
    to_parse.each_byte do |b|
      if b == 10
        @line += 1
        @column = 1
      else
        @column += 1
      end
    end

    :need_more_data
  end

  # Finish parsing - handle remaining buffer
  def finish(&on_event)
    return if @buffer.empty?

    parser = Parser.new(@buffer)
    parser.instance_variable_set(:@line, @line)
    parser.instance_variable_set(:@column, @column)

    parser.parse do |event|
      yield to_stream_event(event, @global_offset)
    end

    @buffer.clear
  end

  private

  def to_stream_event(event, offset)
    # Convert Event to StreamEvent with adjusted spans
    # (implementation details in template)
  end
end
```

### Usage

```ruby
parser = Udon::StreamingParser.new

File.open('large_file.udon', 'rb') do |f|
  f.each(nil, 4096) do |chunk|  # 4KB chunks
    result = parser.parse(chunk) { |event| process(event) }
    break if result == :complete
  end
  parser.finish { |event| process(event) }
end
```

## Verification

1. Generate Ruby parser from a simple test case:
   ```bash
   descent generate examples/lines.desc --target ruby -o test_lines.rb
   ruby -c test_lines.rb  # syntax check
   ```

2. Test with actual input:
   ```ruby
   require_relative 'test_lines'
   Lines::Parser.new("Hello\nWorld\n").parse { |e| p e }
   ```

3. Generate UDON parser and verify syntax:
   ```bash
   cat ~/src/libudon/generator/*.desc | descent generate --target ruby -o udon_parser.rb
   ruby -c udon_parser.rb
   ```

4. Run UDON parser on sample UDON input to verify event stream.

## Scope Summary

Production-ready spike with:
- Full DSL support (all constructs used by UDON)
- Fast scanning via String#index (C-implemented)
- Trace output (--trace flag)
- Streaming/chunked parsing
- Binary string handling
- Line/column tracking

## Benchmark Results

Compared to Ruby's YAML parser (C extension wrapping libyaml):
- ~3x slower for similar data sizes
- Reasonable given: pure Ruby vs C, streaming events vs tree building

For maximum performance, use the Rust-generated parser with Ruby FFI (udon-ruby).

---

## Status (2026-07-08)

**Paused, likely permanently.** The Ruby target reached a working prototype
but surfaced structural debt (the IR bakes Rust byte-literals into what
should be target-neutral form — see the `ruby_expr` reverse-conversion
workaround in generator.rb), landed with no tests, and its own benchmark
conclusion above points back to the Rust path. Direction since: Rust-first;
descent's Rust target verified byte-identical across this detour, so nothing
here affects it. Full assessment and the conditions under which descent gets
further investment (explicit-stack streaming backend, literate-spec merge):
`~/src/udon/REVIEW-JULY-2026.md` §5 and CTQ-E.
