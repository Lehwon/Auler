#let schedule = json("schedule.json")
#let ink = rgb("142b3d")
#let border = rgb("b4c4d1")
#let margin-x = 16mm
#let inset-x = if schedule.grid { 5pt } else { 7pt }
#let body-width = (if schedule.landscape { 297mm } else { 210mm }) - 2 * margin-x
#let full-body-height = (if schedule.landscape { 210mm } else { 297mm }) - 46mm
#set document(title: schedule.title, date: none)
#set text(font: "Malgun Gothic", lang: schedule.language, size: 10pt, fill: ink)
#set page(
  paper: "a4",
  flipped: schedule.landscape,
  margin: (top: 25mm, bottom: 21mm, x: margin-x),
  fill: white,
  numbering: "1 / 1",
)
#set par(leading: 0.55em)
#set heading(numbering: none)

#for section in schedule.sections {
  let context-line = schedule.version + " · " + schedule.title + " · " + section.title
  page(header: align(left, text(size: 9pt, context-line)))[
    #heading(level: 1, schedule.title)
    #heading(level: 2, section.title)
    #context table(
      columns: schedule.column_weights.map(weight => weight * 1fr),
      inset: (x: inset-x, y: 6pt),
      stroke: 0.45pt + border,
      fill: (col, row) => if row == 0 { rgb("e1edf6") } else if schedule.grid and col == 0 or calc.odd(row) { rgb("f6f8fa") },
      table.header(..schedule.headings.map(heading => table.cell(text(heading, weight: "bold")))),
      ..section.rows.flatten().enumerate().map(((index, value)) => {
        let content = value.split("\n").map(line => text(line)).join(linebreak())
        let col = calc.rem(index, schedule.column_weights.len())
        let width = body-width * schedule.column_weights.at(col) / schedule.column_weights.sum() - 2 * inset-x
        // Keep a record together unless the cell is itself larger than the page body.
        table.cell(breakable: measure(content, width: width).height > full-body-height - 24pt, content)
      }),
    )
  ]
}
