# Input encoding diagnostic V1

The additive `*_with_diagnostic` C entry points return one diagnostic from the
same encoding attempt. The older entry points and their status codes are
unchanged. This record describes failures while encoding a semantic input
event; output parser failures continue to use `TL-DIAG/2` or `TL-DIAG/3`.

The diagnostic is ASCII, at most 2048 bytes, and has exactly this field order:

```text
TL-INPUT-DIAG/1
direction=input_encoding
status=unsupported
kind=key
reason=unsupported_control_character
key=Character
codepoint=U+00E9
modifiers=0x04
```

Every line ends with LF, including the last line. `status` is one of
`unsupported`, `invalid_argument`, `limit_exceeded`, `failed_closed`. `kind` is
one of `key`, `kitty_key`, `win32_key`, `paste`, `mouse`, `focus`. `reason` is a
snake_case code derived from `InputEncodeError`; readers should preserve
unknown reason codes for forward compatibility. `key` is `none`, `Character`,
a PascalCase key name, a `Keypad_` name, or a four-digit hexadecimal `VK_XXXX`.
`codepoint` is `none` or an uppercase hexadecimal Unicode scalar prefixed
with `U+` and padded to at least four digits. `modifiers` is `none` or a two
digit hexadecimal mask prefixed with `0x`. Legacy key and mouse masks use
Shift=1, Alt=2, Control=4, Meta=8. Kitty uses Shift=1, Alt=2, Control=4,
Super=8, Hyper=16, Meta=32, CapsLock=64, NumLock=128.

Only a single offending character scalar is exposed as `codepoint`. Paste
content and Kitty associated text are never included. No output sequence,
wire bytes or screen coordinates are fabricated. Structural ABI validation
failures return their existing status with both outputs empty. On success only
`out_encoded` receives a buffer. On semantic encoding failure only
`out_diagnostic` receives a buffer. Both outputs must be independent, empty,
versioned buffers, and each populated output must be released once through
`tl_terminal_buffer_free`.
