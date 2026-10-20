use std::cmp::Ordering;

use druid::{
    widget::{prelude::*, ListIter},
    Data, Point, WidgetPod,
};

/// A wrapping grid of variable-length children, similar to [`List`].
///
/// Children are laid out left-to-right and wrap to the next row. Column count is
/// derived from the available width and `cell_width`.
pub struct Grid<T> {
    closure: Box<dyn Fn() -> Box<dyn Widget<T>>>,
    children: Vec<WidgetPod<T, Box<dyn Widget<T>>>>,
    cell_width: f64,
}

impl<T: Data> Grid<T> {
    pub fn new<W: Widget<T> + 'static>(
        closure: impl Fn() -> W + 'static,
        cell_width: f64,
    ) -> Self {
        Self {
            closure: Box::new(move || Box::new(closure())),
            children: Vec::new(),
            cell_width: cell_width.max(1.0),
        }
    }

    fn update_child_count(&mut self, data: &impl ListIter<T>, _env: &Env) -> bool {
        let len = self.children.len();
        match len.cmp(&data.data_len()) {
            Ordering::Greater => self.children.truncate(data.data_len()),
            Ordering::Less => data.for_each(|_, i| {
                if i >= len {
                    let child = WidgetPod::new((self.closure)());
                    self.children.push(child);
                }
            }),
            Ordering::Equal => {}
        }
        len != data.data_len()
    }
}

impl<C: Data, T: ListIter<C>> Widget<T> for Grid<C> {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut T, env: &Env) {
        let mut children = self.children.iter_mut();
        data.for_each_mut(|child_data, _| {
            if let Some(child) = children.next() {
                child.event(ctx, event, child_data, env);
            }
        });
    }

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, event: &LifeCycle, data: &T, env: &Env) {
        if let LifeCycle::WidgetAdded = event {
            if self.update_child_count(data, env) {
                ctx.children_changed();
            }
        }

        let mut children = self.children.iter_mut();
        data.for_each(|child_data, _| {
            if let Some(child) = children.next() {
                child.lifecycle(ctx, event, child_data, env);
            }
        });
    }

    fn update(&mut self, ctx: &mut UpdateCtx, _old_data: &T, data: &T, env: &Env) {
        let mut children = self.children.iter_mut();
        data.for_each(|child_data, _| {
            if let Some(child) = children.next() {
                child.update(ctx, child_data, env);
            }
        });

        if self.update_child_count(data, env) {
            ctx.children_changed();
        }
    }

    fn layout(&mut self, ctx: &mut LayoutCtx, bc: &BoxConstraints, data: &T, env: &Env) -> Size {
        let width = if bc.max().width.is_finite() {
            bc.max().width
        } else {
            self.cell_width
        };
        let cols = ((width / self.cell_width).floor() as usize).max(1);
        let cell_w = if cols == 1 {
            width.max(1.0)
        } else {
            self.cell_width
        };
        let child_bc = BoxConstraints::new(Size::new(0.0, 0.0), Size::new(cell_w, f64::INFINITY));

        let child_count = self.children.len();
        let mut children = self.children.iter_mut();
        let mut child_sizes = Vec::with_capacity(child_count);
        data.for_each(|child_data, _| {
            if let Some(child) = children.next() {
                let size = child.layout(ctx, &child_bc, child_data, env);
                child_sizes.push(size);
            }
        });

        let mut y = 0.0;
        let mut max_row_height;
        let mut i = 0;
        while i < child_sizes.len() {
            let row_end = (i + cols).min(child_sizes.len());
            max_row_height = child_sizes[i..row_end]
                .iter()
                .map(|s| s.height)
                .fold(0.0, f64::max);

            for (col, child) in self.children[i..row_end].iter_mut().enumerate() {
                child.set_origin(ctx, Point::new(col as f64 * cell_w, y));
            }

            y += max_row_height;
            i = row_end;
        }

        bc.constrain(Size::new(width, y))
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        let mut children = self.children.iter_mut();
        data.for_each(|child_data, _| {
            if let Some(child) = children.next() {
                child.paint(ctx, child_data, env);
            }
        });
    }
}
