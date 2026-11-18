use super::ImportError;
use chrono::{Datelike, Months, NaiveDate, Utc};
use std::time::SystemTime;

const MAX_OBJECTS: usize = 50;
const MAX_MONTHS: u32 = 60;

/// A month-aligned, start-inclusive, end-exclusive GP History range.
#[derive(Debug)]
pub struct HistoryPlan {
    start: NaiveDate,
    end: NaiveDate,
    ids: Vec<u32>,
    pub(super) scope: String,
}

impl HistoryPlan {
    /// An empty ID list is valid for preview. Live acquisition requires IDs.
    pub fn new(start: &str, end: &str, mut ids: Vec<u32>) -> Result<Self, ImportError> {
        let start = month(start, "start must be a UTC month in YYYY-MM format")?;
        let end = month(end, "end must be a UTC month in YYYY-MM format")?;
        if start >= end {
            return Err(ImportError::Plan(
                "start must precede the exclusive end month",
            ));
        }
        if start
            .checked_add_months(Months::new(MAX_MONTHS))
            .is_some_and(|limit| end > limit)
        {
            return Err(ImportError::Plan("a plan may cover at most 60 months"));
        }
        ids.sort_unstable();
        ids.dedup();
        if ids.contains(&0) || ids.len() > MAX_OBJECTS {
            return Err(ImportError::Plan(
                "use at most 50 distinct positive NORAD IDs",
            ));
        }
        let scope = ids.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
        Ok(Self {
            start,
            end,
            ids,
            scope,
        })
    }

    pub fn start(&self) -> NaiveDate {
        self.start
    }

    pub fn end(&self) -> NaiveDate {
        self.end
    }

    pub fn ids(&self) -> &[u32] {
        &self.ids
    }

    pub(super) fn windows(&self) -> impl Iterator<Item = MonthWindow> + '_ {
        std::iter::successors(Some(self.start), |month| {
            month.checked_add_months(Months::new(1))
        })
        .take_while(|month| *month < self.end)
        .map(|start| MonthWindow {
            start,
            end: start
                .checked_add_months(Months::new(1))
                .expect("each window ends within the plan"),
        })
    }

    pub fn validate_live(&self) -> Result<(), ImportError> {
        if self.ids.is_empty() {
            return Err(ImportError::Plan(
                "live GP History import requires explicit NORAD IDs",
            ));
        }
        let now = chrono::DateTime::<Utc>::from(SystemTime::now()).date_naive();
        if self.end > now.with_day(1).expect("every month has a first day") {
            return Err(ImportError::Plan(
                "GP History import uses completed UTC months only",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonthWindow {
    pub start: NaiveDate,
    pub end: NaiveDate,
}

impl MonthWindow {
    pub(super) fn key(self) -> String {
        self.start.format("%Y-%m").to_string()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PageQuery<'a> {
    pub window: MonthWindow,
    pub object_scope: &'a str,
    pub after_gp_id: i64,
    pub limit: usize,
}

impl PageQuery<'_> {
    fn request_path(&self) -> String {
        let last_day = self
            .window
            .end
            .pred_opt()
            .expect("a month follows its final day");
        format!(
            "/basicspacedata/query/class/gp_history/NORAD_CAT_ID/{}/EPOCH/{}%2000:00:00--{}%2023:59:59.999999/GP_ID/%3E{}/orderby/GP_ID%20asc/limit/{}/format/json/emptyresult/show",
            self.object_scope, self.window.start, last_day, self.after_gp_id, self.limit
        )
    }

    pub(super) fn url(&self) -> String {
        format!("https://www.space-track.org{}", self.request_path())
    }
}

fn month(value: &str, message: &'static str) -> Result<NaiveDate, ImportError> {
    let parsed = NaiveDate::parse_from_str(&format!("{value}-01"), "%Y-%m-%d")
        .map_err(|_| ImportError::Plan(message))?;
    if value.len() != 7 || parsed.year() < 1 || parsed.format("%Y-%m").to_string() != value {
        return Err(ImportError::Plan(message));
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_has_an_inclusive_microsecond_range_and_a_stable_page_cursor() {
        let plan = HistoryPlan::new("2020-02", "2020-03", vec![43013, 25544, 25544]).unwrap();
        let query = PageQuery {
            window: plan.windows().next().unwrap(),
            object_scope: &plan.scope,
            after_gp_id: 99,
            limit: 20,
        };
        assert_eq!(
            query.url(),
            "https://www.space-track.org/basicspacedata/query/class/gp_history/NORAD_CAT_ID/25544,43013/EPOCH/2020-02-01%2000:00:00--2020-02-29%2023:59:59.999999/GP_ID/%3E99/orderby/GP_ID%20asc/limit/20/format/json/emptyresult/show"
        );
    }
}
