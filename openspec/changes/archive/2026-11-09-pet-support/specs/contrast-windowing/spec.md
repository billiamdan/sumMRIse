## ADDED Requirements

### Requirement: Uptake presets
The preset menu SHALL list, for a series whose values are standardised uptake values
(unit "SUV"), the stored windows (converted to SUV), then the built-in windows "SUV 0–5",
"SUV 0–7", "SUV 0–10" and "SUV 0–15", each spanning from SUV 0 to its upper limit,
followed by Auto and Full range; no CT preset is offered. Keys 1–9 and A keep their
meaning. A series in SUV that stores no window SHALL open with "SUV 0–5" instead of the
automatic window. The W and L fields and the annotations SHALL show SUV with up to two
decimals.

#### Scenario: PET series in SUV opens with SUV 0–5
- **WHEN** a PET series in SUV that stores no window is opened
- **THEN** it is shown with the window "SUV 0–5" (W 5 / L 2.5 SUV), and Auto is still offered in the menu

#### Scenario: PET series in SUV without stored windows
- **WHEN** a PET series in SUV without stored windows is displayed and the user presses 2
- **THEN** the window "SUV 0–7" is applied (W 7 / L 3.5 SUV): SUV 0 is at one end of the grey scale and SUV 7 and above at the other

#### Scenario: PET series kept in Bq/ml
- **WHEN** SUV cannot be computed for a PET series
- **THEN** no SUV preset is offered

### Requirement: Colour maps
For PET and nuclear-medicine series the display intensity SHALL be shown through a
selectable colour map of 256 entries, indexed by the display level of the window: Grey
(black to white), Inverted grey (white to black; the default), Hot iron (black through
red, orange and yellow to (255, 254, 252)) and Rainbow (black through blue, cyan, green
and yellow to red). Pixels outside a reformatted volume SHALL stay black with every
colour map, and inversion (I) SHALL apply before the colour map.

#### Scenario: End points
- **WHEN** the window is SUV 0–5 and the colour map is Rainbow
- **THEN** SUV 0 is black, SUV 5 and above is red (255, 0, 0), and SUV 2 (two fifths of the window) is cyan (0, 255, 255)
