## ADDED Requirements

### Requirement: Contrast tables
Every table of the VOI LUT Sequence SHALL be offered as a contrast choice named by its LUT
Explanation (title-cased, or "Contrast table n"), listed after the stored windows and
before the built-in windows, Auto and Full range, with keys 1–9 counting through that list;
the first table SHALL be the default when the image stores no window; a table SHALL map the
modality value (rounded; first or last entry outside its input range) to grey in proportion
to its entry (of 2^bits − 1 of its descriptor), applied to acquired images only, while
reformats and slabs keep the window; choosing a window, dragging, editing W or L, or A SHALL
switch to an ordinary window; where stacks keep a window each they SHALL keep a table choice
each; reset SHALL restore the default.

#### Scenario: Image with two tables and no window
- **WHEN** a radiograph without window values has the tables NORMAL (entry i = round(4095 × √(i / 4095)), 12 bits) and HARDER
- **THEN** it opens with "Normal", the menu offers "Normal" and "Harder" first (keys 1 and 2), and a pixel with stored value 100 is drawn with grey level 40 of 255 (entry 640 of 4095)

#### Scenario: Adjusting contrast by hand
- **WHEN** the user drags with the contrast tool while "Normal" is active
- **THEN** the image is shown with an ordinary window that the drag changes, and the window is labelled "Custom window"

#### Scenario: Read-out with a table
- **WHEN** the pointer is over the pixel with stored value 100 while "Harder" is active
- **THEN** the read-out shows 100, the value, not the table's output
