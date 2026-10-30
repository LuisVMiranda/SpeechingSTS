---
name: Speeching
description: A compact, microphone-first interface for spoken language practice.
colors:
  light-page: "#edf2f8"
  light-surface: "#fff"
  light-ink: "#182439"
  light-muted: "#52637c"
  light-line: "#d9e1ec"
  light-accent: "#2768ed"
  light-accent-strong: "#1853c8"
  light-soft: "#eef4ff"
  dark-page: "#0e1724"
  dark-surface: "#182536"
  dark-ink: "#e9f0fa"
  dark-muted: "#aabbd0"
  dark-line: "#39495d"
  dark-accent: "#578bff"
  dark-accent-strong: "#7ba3ff"
  dark-soft: "#233a5c"
  microphone-active: "#a63847"
typography:
  body:
    fontFamily: '"Segoe UI", system-ui, sans-serif'
  brand:
    fontFamily: '"Segoe UI", system-ui, sans-serif'
    fontSize: "1.03rem"
    fontWeight: 700
    letterSpacing: "-0.025em"
  heading:
    fontFamily: '"Segoe UI", system-ui, sans-serif'
    fontSize: "1.08rem"
    letterSpacing: "-0.025em"
  field-label:
    fontFamily: '"Segoe UI", system-ui, sans-serif'
    fontSize: "0.76rem"
    fontWeight: 650
  primary-button:
    fontFamily: '"Segoe UI", system-ui, sans-serif'
    fontSize: "0.85rem"
    fontWeight: 700
rounded:
  quiet-button: "8px"
  field: "9px"
  message: "10px"
  icon-button: "11px"
  panel: "16px"
  circle: "50%"
spacing:
  control-gap: "8px"
  message-inset: "9px 12px"
  button-inset: "11px 16px"
  panel-inset: "23px"
components:
  button-primary:
    backgroundColor: "{colors.light-accent}"
    textColor: "{colors.light-surface}"
    rounded: "{rounded.field}"
    padding: "{spacing.button-inset}"
    typography: "{typography.primary-button}"
  button-primary-hover:
    backgroundColor: "{colors.light-accent-strong}"
  microphone:
    backgroundColor: "{colors.light-accent}"
    textColor: "{colors.light-surface}"
    rounded: "{rounded.circle}"
    size: "132px"
  microphone-active:
    backgroundColor: "{colors.microphone-active}"
  field-light:
    backgroundColor: "{colors.light-surface}"
    textColor: "{colors.light-ink}"
    rounded: "{rounded.field}"
    padding: "9px 11px"
  field-dark:
    backgroundColor: "{colors.dark-surface}"
    textColor: "{colors.dark-ink}"
    rounded: "{rounded.field}"
    padding: "9px 11px"
  message-light:
    backgroundColor: "{colors.light-soft}"
    rounded: "{rounded.message}"
    padding: "{spacing.message-inset}"
  message-dark:
    backgroundColor: "{colors.dark-soft}"
    rounded: "{rounded.message}"
    padding: "{spacing.message-inset}"
---

# Design System: Speeching

## Overview

**Creative North Star: "Compact voice practice"** is a retrospective description of the existing interface, not a new visual direction. A prominent blue microphone anchors a quiet, compact conversation surface; utilities, transcripts, and settings stay close at hand. Light and dark themes share the same shapes and hierarchy.

**Key Characteristics:**

- A single centered practice panel and a large circular microphone.
- Cool neutrals with a blue action color and restrained rounded controls.
- Dense, readable controls with visible focus and reduced-motion support.

## Colors

Blue identifies the microphone, primary actions, focus, and interactive emphasis. Cool neutral surfaces and text change with the selected theme. The microphone, waveform, brand mark, and primary buttons use the fixed light-accent blue in both themes; secondary accents use the theme-specific accent tokens. The active microphone changes to the recorded red.

**The Theme Roles Rule.** Use the matching page, surface, ink, muted, line, accent, and soft colors together for each theme.

## Typography

The interface uses the Segoe UI system stack. Field labels and compact controls use small, weighted text; section headings are modest rather than display-sized. The brand uses the original gem-textured Speeching icon.

The frontmatter records the recurring brand, section-heading, field-label, and primary-button styles. Transcript messages use a readable line height (1.45); empty-state text uses a slightly more open line height (1.5). Timers and history metadata use tabular numerals.

## Layout

The outer shell is centered and capped at 1030px. The practice surface caps at 500px; an open drawer adds a second column, up to 430px wide, with an 18px gap. The header stays aligned to the full workspace width so its controls do not move when a drawer opens. A quiet footer follows the page content. At 800px and below, the drawer stacks below the practice surface. At 390px and below, settings and history controls collapse to one column, horizontal panel padding tightens, and the microphone becomes 120px.

The practice panel places the conversation selector above the centered microphone, status, timer, waveform, and transcript. History and settings use the same inset panel language and expand with the page. The live transcript scrolls within a bounded height so incoming speech does not move the page.

## Elevation & Depth

Large panels use a diffuse theme-specific shadow; thin borders and soft fills separate controls and transcript messages. A double ring surrounds the microphone. The floating notice has its own smaller shadow. Exact shadow values are recorded in the sidecar.

## Shapes

Panels have rounded corners, inputs and buttons use smaller radii, and the microphone is circular. Dividers and control borders are thin and use the current theme's line color. The practice panel clips its content to its rounded boundary.

## Components

- **Microphone:** A fixed-blue circular button with a white outlined icon and a double ring. Hover enlarges it slightly and darkens the blue; press slightly shrinks it. The active state changes its fill and outer ring to red. Its transition lasts 0.2 seconds.
- **Buttons:** Primary buttons have a blue fill and white text. Utility buttons are bordered squares, while secondary actions use quiet bordered or text-only treatments. Disabled buttons reduce opacity.
- **Fields:** Inputs and selects use the current surface, ink, and border colors, with a minimum height of 42px. Labels sit above their controls.
- **Transcript:** Assistant messages use the soft theme fill; user messages use a transparent fill and a thin border. Both share rounded corners and wrap long content.
- **History and settings:** A rounded, shadowed drawer holds compact controls and divided sections. The page scrolls to reveal longer content. Conversation rows change to the stronger accent when hovered or selected.
- **Activity:** The blue waveform gains opacity and animates while active. Reduced-motion preferences reduce animation and transition durations.

**The Visible Focus Rule.** Buttons, inputs, and selects show a 3px theme-accent outline with a 3px offset on keyboard focus.

## Do's and Don'ts

### Do:

- **Do** pair light and dark theme roles for surfaces, text, dividers, and secondary accents.
- **Do** keep the microphone and connection status prominent in the practice surface.
- **Do** preserve visible keyboard focus and support reduced motion.

### Don't:

- **Don't** remove the text status when presenting the animated activity waveform.
- **Don't** prevent the drawer and form controls from stacking on narrow screens.
