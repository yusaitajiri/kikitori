// The look of a Kikitori transcript (export/typst_pdf.rs). The session follows this preamble as
// calls to `header`, `entry`, `shot`, `note` and `cut`, its text in strings, so none of it is read as
// markup. White paper, ink, and red for the voice you hear (相手).

#let ink = rgb("#121214")
#let muted = rgb("#66666d")
#let red = rgb("#d8232a")
#let hairline = rgb("#e3e3e6")

// The app icon's bird, r its head's radius: a red head with a pointed tail, an ink beak and a
// white eye. It stands on the baseline like a letter.
#let bird(r) = box(width: 2.99 * r, height: 2 * r, {
  place(dx: 0.5348 * r, circle(radius: r, fill: red))
  place(polygon(fill: red, (0pt, 0.3167 * r), (1.3178 * r, 0.0238 * r), (0.6641 * r, 1.492 * r)))
  place(polygon(fill: ink, (2.4249 * r, 1.0151 * r), (2.9902 * r, 0.6371 * r), (2.3136 * r, 0.5688 * r)))
  place(dx: 1.8548 * r, dy: 0.56 * r, circle(radius: 0.1 * r, fill: white))
})

#set page(
  paper: "a4",
  margin: (x: 18mm, top: 18mm, bottom: 20mm),
  // The app's name, small, with its bird; the page number on the right.
  footer: context {
    set text(6.5pt, fill: muted)
    grid(
      columns: (1fr, 1fr),
      align: (left + horizon, right + horizon),
      [#bird(1.7pt)#h(0.8pt) Kikitori],
      counter(page).display("1 / 1", both: true),
    )
  },
)
#set text(font: ("Yu Gothic", "Meiryo", "MS Gothic"), size: 10.5pt, lang: "ja", fill: ink)
#set par(leading: 0.85em, spacing: 0.95em)

// The title, then the date, length and sources.
#let header(title, meta) = {
  text(17pt, weight: "bold", title)
  v(-0.3em)
  text(9pt, fill: muted, meta)
  v(0.2em)
  line(length: 100%, stroke: 0.6pt + ink)
  v(0.4em)
}

// One paragraph: its time and who spoke (both optional), then what was said.
#let entry(time: none, label: none, me: false, body) = par({
  if time != none { text(fill: muted, number-width: "tabular", "[" + time + "]") + h(0.4em) }
  if label != none { text(weight: "bold", fill: if me { ink } else { red }, label) + h(0.4em) }
  body
})

// A screenshot, its width chosen so a tall one stays on the page, with its caption.
#let shot(path, width, caption) = block(breakable: false, above: 1em, below: 1.1em, {
  box(stroke: 0.5pt + hairline, image(path, width: width))
  v(-0.4em)
  text(9pt, fill: muted, caption)
})

// A pause, a reconnected source or audio left unprocessed.
#let note(body) = align(center, text(9.5pt, fill: muted, style: "italic", body))

// A cut: a new part of the session, its time over a hairline. A heading, so the PDF's outline
// lists the parts.
#show heading.where(level: 2): it => block(above: 1.6em, below: 0.9em, sticky: true, {
  set text(9.5pt, weight: "bold", fill: muted, number-width: "tabular")
  it.body
  v(-0.55em)
  line(length: 100%, stroke: 0.5pt + hairline)
})
#let cut(time) = heading(level: 2, time)
