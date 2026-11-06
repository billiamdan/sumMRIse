## ADDED Requirements

### Requirement: Display check
The application SHALL offer a display check, opened from the settings window and from the
About dialog and closed with Escape or Close: a picture generated in memory for the pixel
size of the area it fills and drawn 1:1 without smoothing — grey steps 0, 10 … 100 %, a 5 %
square inside the 0 % step and a 95 % square inside the 100 % step, a continuous ramp from
black to white, fine line pairs and a border — with plain instructions on what should be
visible and what to change if it is not, described as a quick check in the style of the
SMPTE pattern and not as a calibration or a compliance claim; it SHALL involve no patient
data.

#### Scenario: Patches at their pixels
- **WHEN** the picture is generated for 800 × 600 pixels
- **THEN** the 0 % step reads 0 around the 5 % square, which spans columns 43–72 and rows 102–131 and reads 13, the 50 % step reads 128 at column 399, row 117, and the 95 % square reads 242

#### Scenario: Another window size
- **WHEN** the picture is generated for 1280 × 720 pixels
- **THEN** every step reads round(p × 255) for its level p at its centre, and the outermost pixel row is white

#### Scenario: Opening from the settings
- **WHEN** the user presses Ctrl/Cmd+comma, moves to "Check the display…" with Tab and presses Space
- **THEN** the display check covers the window with its instructions, including "It is not a calibration", and Escape closes it
