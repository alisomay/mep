# API Reference for `mep`

`mep` uses koto_midi's functions and utilities for MIDI processing.

Every event processing script should start with `import midi`.

Every event processing script should define a `midi.listen` function.

Ex.

```coffee
# Import the midi module to have access to the midi map.
import midi

midi.listen = |incoming_midi_message|
  # Here you may process the `incoming_midi_message`,
  # which is a list of integers. Ex. `[0x90, 0x3C, 0x7F]` or `[144, 120, 111]`

  # If you wish to send it through the output port, use `midi.send`.
  midi.send incoming_midi_message
  ()

```

## Members of the `midi` map

Bring `midi` in the scope by starting your script with `import midi`.

---

### `midi.types`

---

note_off : `"note_off"`

note_on : `"note_on"`

poly_after_touch : `"poly_after_touch"`

control_change : `"control_change"`

program_change : `"program_change"`

after_touch : `"after_touch"`

pitch_bend : `"pitch_bend"`

all_sound_off : `"all_sound_off"`

reset_all_controllers : `"reset_all_controllers"`

local_control : `"local_control"`

all_notes_off : `"all_notes_off"`

omni_mode_off : `"omni_mode_off"`

omni_mode_on : `"omni_mode_on"`

mono_mode_on : `"mono_mode_on"`

poly_mode_on : `"poly_mode_on"`

system_exclusive : `"system_exclusive"`

time_code_quarter_frame : `"time_code_quarter_frame"`

song_position : `"song_position"`

song_select : `"song_select"`

tune_request : `"tune_request"`

end_of_exclusive : `"end_of_exclusive"`

timing_clock : `"timing_clock"`

start : `"start"`

continue : `"continue"`

stop : `"stop"`

active_sensing : `"active_sensing"`

reset : `"reset"`

undefined : `"undefined"`

malformed : `"malformed"`

---

### `midi.categories`

---

channel_voice : `"channel_voice"`

channel_mode : `"channel_mode"`

system_common : `"system_common"`

system_realtime : `"system_realtime"`

unknown : `"unknown"`

---

### `midi.parse` -> `|| -> <a member of midi.message>`

---

This function expects a single list of positive integers as its argument. Ex. `[0x90, 0x3C, 0x7F]` `[144, 120, 111]`

It will return a message map. One of the types in `midi.message` or throw a runtime error.

---

### `midi.send` -> `|[<byte>, ..]| -> ()`

---

Sends a list of bytes through the midi output port.

Arguments are internally checked for bounds and types.

It will throw an error if the members of the list is not in the range of a byte, which is `0..=255`.

---

### `midi.message`

---

This map is a collection of midi message constructors.

All constructors **return a map or an empty value**.

### Shared properties of the returned map among all constructors are,

```
<base-message-map>
  category : <a member of midi.categories>
  type : <a member of midi.types>
  # A method to pack a sendable midi message
  pack : || -> [<integers>, ..]
```

### List of constructors

| `...` notation in the return types is a place holder for `<base-message-map>`'s properties.

---

note_off `|[<note>, <velocity>, <channel>]| -> <note-off-message-map> | ()`

```
<note-off-message-map>
  note : 0..=127
  velocity : 0..=127
  channel : 0..=15
  ...
```

note_on `|[<note>, <velocity>, <channel>]| -> <note-on-message-map> | ()`

```
<note-on-message-map>
  note : 0..=127
  velocity : 0..=127
  channel : 0..=15
  ...
```

poly_after_touch `|[<note>, <pressure>, <channel>]| -> <poly-after-touch-message-map> | ()`

```
<poly-after-touch-message-map>
  note : 0..=127
  pressure : 0..=127
  channel : 0..=15
  ...
```

control_change `|[<note>, <value>, <channel>]| -> <control-change-message-map> | ()`

```
<control-change-message-map>
  note : 0..=127
  value : 0..=127
  channel : 0..=15
  ...
```

program_change `|[<program>, <channel>]| -> <program-change-message-map> | ()`

```
<program-change-message-map>
  program : 0..=127
  channel : 0..=15
  ...
```

after_touch `|[<pressure>, <channel>]| -> <after-touch-message-map> | ()`

```
<after-touch-message-map>
  pressure : 0..=127
  channel : 0..=15
  ...
```

pitch_bend `|[<bend_amount>, <channel>]| -> <pitch-bend-message-map> | ()`

```
<pitch-bend-message-map>
  bend_amount : 0..=16383
  channel : 0..=15
  ...
```

all_sound_off `|[<value>, <channel>]| -> <all-sound-off-message-map> | ()`

```
<all-sound-off-message-map>
  note : 120
  value : 0..=127
  channel : 0..=15
  ...
```

reset_all_controllers `|[<value>, <channel>]| -> <reset-all-controllers-message-map> | ()`

```
<reset-all-controllers-message-map>
  note : 121
  value : 0..=127
  channel : 0..=15
  ...
```

local_control `|[<value>, <channel>]| -> <local-control-message-map> | ()`

```
<local-control-message-map>
  note : 122
  value : 0..=127
  channel : 0..=15
  ...
```

all_notes_off `|[<value>, <channel>]| -> <all-notes-off-message-map> | ()`

```
<all-notes-off-message-map>
  note : 123
  value : 0..=127
  channel : 0..=15
  ...
```

omni_mode_off `|[<value>, <channel>]| -> <omni-mode-off-message-map> | ()`

```
<omni-mode-off-message-map>
  note : 124
  value : 0..=127
  channel : 0..=15
  ...
```

omni_mode_on `|[<value>, <channel>]| -> <omni-mode-on-message-map> | ()`

```
<omni-mode-on-message-map>
  note : 125
  value : 0..=127
  channel : 0..=15
  ...
```

mono_mode_on `|[<value>, <channel>]| -> <mono-mode-on-message-map> | ()`

```
<mono-mode-on-message-map>
  note : 126
  value : 0..=127
  channel : 0..=15
  ...
```

poly_mode_on `|[<value>, <channel>]| -> <poly-mode-on-message-map> | ()`

```
<poly-mode-on-message-map>
  note : 127
  value : 0..=127
  channel : 0..=15
  ...
```

time_code_quarter_frame `|[<message_type>, <values>]| -> <time-code-quarter-frame-message-map> | ()`

```
<time-code-quarter-frame-message-map>
  message_type : 0..=7
  values : 0..=15
  ...
```

song_position `|[<midi_beats_elapsed>]| -> <song-position-message-map> | ()`

```
<song-position-message-map>
  midi_beats_elapsed: 0..=16383
  ...
```

song_select `|[<number>]| -> <song-select-message-map> | ()`

```
<song-select-message-map>
  number: 0..=127
  ...
```

tune_request `|| -> <base-message-map>`

end_of_exclusive `|| -> <base-message-map>`

system_exclusive `|[<manufacturer_id>, <message>]| -> <system-exclusive-message-map>`

```
<byte>: 0..=255
<system-exclusive-message-map>
  manufacturer_id: [<byte>] | [<byte>,<byte>,<byte>]
  # Message can be any length
  message: [<byte>, ..]
  ...
```

timing_clock `|| -> <base-message-map>`

start `|| -> <base-message-map>`

continue `|| -> <base-message-map>`

stop `|| -> <base-message-map>`

active_sensing `|| -> <base-message-map>`

reset `|| -> <base-message-map>`

malformed `|| -> <base-message-map>`

undefined `|| -> <base-message-map>`
