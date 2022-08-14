use std::{collections::HashMap, marker::PhantomData};

use papergrid::{
    records::{Records, RecordsMut},
    width::CfgWidthFunction,
    Entity, GridConfig,
};

use crate::{
    width::{
        count_borders, get_table_widths_with_total, get_width_value, ColumnPeaker, PriorityNone,
        WidthValue,
    },
    CellOption, Table, TableOption,
};

/// [`MinWidth`] changes a content in case if it's length is lower then the boundary.
///
/// It can be applied to a whole table.
///
/// It does anything in case if the content's length is bigger then the boundary.
/// It doesn't include a [`Padding`] settings.
///
/// Be aware that it doesn't consider padding.
/// So if you want to set a exact width you might need to use [`Padding`] to set it to 0.
///
/// ## Examples
///
/// Cell change
///
/// ```
/// use tabled::{object::Segment, width::MinWidth, Modify, Style, Table};
///
/// let data = ["Hello", "World", "!"];
///
/// let table = Table::new(&data)
///     .with(Style::markdown())
///     .with(Modify::new(Segment::all()).with(MinWidth::new(10)));
/// ```
/// Table change
///
/// ```
/// use tabled::{width::MinWidth, Table};
///
/// let table = Table::new(&["Hello World!"]).with(MinWidth::new(5));
/// ```
///
/// [`Padding`]: crate::Padding
#[derive(Debug)]
pub struct MinWidth<W = usize, P = PriorityNone> {
    width: W,
    fill: char,
    _priority: PhantomData<P>,
}

impl<W> MinWidth<W>
where
    W: WidthValue,
{
    /// Creates a new instance of [`MinWidth`].
    pub fn new(width: W) -> Self {
        Self {
            width,
            fill: ' ',
            _priority: PhantomData::default(),
        }
    }
}

impl<W, P> MinWidth<W, P> {
    /// Set's a fill character which will be used to fill the space
    /// when increasing the length of the string to the set boundary.
    pub fn fill_with(mut self, c: char) -> Self {
        self.fill = c;
        self
    }

    /// Priority defines the logic by which a increase of width will be applied when is done for the whole table.
    ///
    /// - [`PriorityNone`] which inc the columns one after another.
    /// - [`PriorityMax`] inc the biggest columns first.
    /// - [`PriorityMin`] inc the lowest columns first.
    pub fn priority<PP: ColumnPeaker>(self) -> MinWidth<W, PP> {
        MinWidth {
            fill: self.fill,
            width: self.width,
            _priority: PhantomData::default(),
        }
    }
}

impl<W, R> CellOption<R> for MinWidth<W>
where
    W: WidthValue,
    R: Records + RecordsMut<String>,
{
    fn change_cell(&mut self, table: &mut Table<R>, entity: Entity) {
        let width_ctrl = CfgWidthFunction::from_cfg(table.get_config());
        let width = self
            .width
            .width(table.get_records(), table.get_config(), &width_ctrl);

        let (count_rows, count_cols) = table.shape();
        for pos in entity.iter(count_rows, count_cols) {
            let records = table.get_records();
            let cell_width = records.get_width(pos, &width_ctrl);
            if cell_width >= width {
                continue;
            }

            let content = records.get_text(pos);
            let content = increase_width(content, width, self.fill);
            let records = table.get_records_mut();
            records.set(pos, content, &width_ctrl);
        }
    }
}

impl<W, P, R> TableOption<R> for MinWidth<W, P>
where
    W: WidthValue,
    P: ColumnPeaker,
    R: Records + RecordsMut<String>,
{
    fn change(&mut self, table: &mut Table<R>) {
        if table.is_empty() {
            return;
        }

        let width = get_width_value(&self.width, table);
        let (widths, total_width) =
            get_table_widths_with_total(table.get_records(), table.get_config());
        if total_width >= width {
            return;
        }

        increase_total_width(table, widths, total_width, width, P::create());
    }
}

#[cfg(not(feature = "color"))]
fn increase_width(s: &str, width: usize, fill_with: char) -> String {
    use papergrid::util::string_width;

    s.lines()
        .map(|line| {
            let length = string_width(line);
            if width > length {
                let remain = width - length;
                let mut new_line = String::with_capacity(width);
                new_line.push_str(line);
                new_line.extend(std::iter::repeat(fill_with).take(remain));
                std::borrow::Cow::Owned(new_line)
            } else {
                std::borrow::Cow::Borrowed(line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(feature = "color")]
fn increase_width(s: &str, width: usize, fill_with: char) -> String {
    use papergrid::util::string_width;

    ansi_str::AnsiStr::ansi_split(s, "\n")
        .map(|line| {
            let length = string_width(&line);
            if length < width {
                let mut line = line.into_owned();
                let remain = width - length;
                line.extend(std::iter::repeat(fill_with).take(remain));
                std::borrow::Cow::Owned(line)
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn increase_total_width<P, R>(
    table: &mut Table<R>,
    widths: Vec<usize>,
    total_width: usize,
    expected_width: usize,
    priority: P,
) where
    P: ColumnPeaker,
    R: Records + RecordsMut<String>,
{
    let records = table.get_records();
    let cfg = table.get_config();
    let increase_list =
        get_increase_total_width_list(records, cfg, widths, expected_width, total_width, priority);

    for ((row, col), width) in increase_list {
        MinWidth::new(width).change_cell(table, (row, col).into());
    }
}

fn get_increase_total_width_list<F, R>(
    records: R,
    cfg: &GridConfig,
    mut widths: Vec<usize>,
    total_width: usize,
    mut width: usize,
    mut peaker: F,
) -> HashMap<(usize, usize), usize>
where
    F: ColumnPeaker,
    R: Records,
{
    while width != total_width {
        let col = match peaker.peak(&[], &widths) {
            Some(col) => col,
            None => break,
        };

        widths[col] += 1;
        width += 1;
    }

    let (count_rows, count_cols) = (records.count_rows(), records.count_columns());
    let mut points = HashMap::new();
    #[allow(clippy::needless_range_loop)]
    for row in 0..count_rows {
        let mut col = 0;
        while col < widths.len() {
            let this_col = col;
            let width = match cfg.get_column_span((row, col)) {
                Some(span) => {
                    let width = (col..col + span).map(|i| widths[i]).sum::<usize>();
                    let count_borders = count_borders(cfg, col, col + span, count_cols);
                    let width = width + count_borders;

                    col += span;

                    width
                }
                None => {
                    col += 1;
                    widths[this_col]
                }
            };

            let padding = cfg.get_padding((row, this_col).into());
            let width = width.saturating_sub(padding.left.size + padding.right.size);

            points.insert((row, this_col), width);
        }
    }

    points
}
