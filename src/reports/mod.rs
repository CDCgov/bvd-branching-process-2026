use crate::parameters::ContextParametersExt;
use crate::timekeeping::TimeKeeper;
use anyhow::{bail, Result};
use ixa::{info, Context};
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};

pub mod confirmation_incidence_report;
pub mod prevalence_report;
pub mod rt_report;
pub mod symptom_onset_report;
pub mod transmission_report;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub enum ReportTrigger {
    Time { time: OrderedFloat<f64> },
    Shutdown,
    SurveillanceCampaign { delay: OrderedFloat<f64> },
    Date { date: TimeKeeper },
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
pub struct ReportParams {
    pub write: bool,
    pub filename: Option<String>,
    pub period: Option<f64>,
    pub trigger: Option<ReportTrigger>,
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
pub struct Reports {
    pub transmission_chain_report: ReportParams,
    pub prevalence_report: ReportParams,
    pub confirmation_incidence_report: ReportParams,
    pub symptom_onset_report: ReportParams,
    pub rt_report: ReportParams,
    pub offspring_distribution_report: ReportParams,
}

impl Reports {
    /// `ReportParams` defaults to `write: false`
    pub fn disabled() -> Self {
        Self::default()
    }
}

fn get_report_name(params: &ReportParams) -> Result<Option<&str>> {
    if params.write {
        if let Some(name) = &params.filename {
            return Ok(Some(name));
        }

        bail!("Reports must be provided with a name when write is set to true");
    }

    if let Some(name) = &params.filename {
        info!("Report {name} is off but has associated values with name.");
    }
    Ok(None)
}

fn get_period_report_name(params: &ReportParams) -> Result<Option<(&str, f64)>> {
    if let Some(name) = get_report_name(params)? {
        if let Some(period) = params.period {
            if period <= 0.0 {
                bail!("The report period must be greater than zero, found period {period} for {name} instead.");
            }
            return Ok(Some((name, period)));
        }

        bail!("Report {name} requires a period but none provided");
    }
    Ok(None)
}

fn get_report_trigger(params: &ReportParams) -> Result<Option<ReportTrigger>> {
    if let Some(name) = get_report_name(params)? {
        if let Some(trigger) = &params.trigger {
            return Ok(Some(trigger.clone()));
        }

        bail!("Report {name} requires a trigger but none provided");
    }
    Ok(None)
}

/// # Errors
///
/// Returns an error if any report within the reports list cannot be added
/// or if the period for any periodic report is less than 0.0.
pub fn init(context: &mut Context) -> Result<()> {
    let Reports {
        prevalence_report,
        confirmation_incidence_report,
        transmission_chain_report,
        symptom_onset_report,
        rt_report,
        offspring_distribution_report,
    } = context.get_params().reports.clone();
    let mut report_count = 0;

    if let Some((name, period)) = get_period_report_name(&prevalence_report)? {
        prevalence_report::init(context, name, period)?;
        info!("Generating the prevalence report.");
        report_count += 1;
    }
    if let Some((name, period)) = get_period_report_name(&confirmation_incidence_report)? {
        confirmation_incidence_report::init(context, name, period)?;
        info!("Generating the incidence report.");
        report_count += 1;
    }
    if let Some(name) = get_report_name(&transmission_chain_report)? {
        transmission_report::init(context, name)?;
        info!("Generating the transmission report.");
        report_count += 1;
    }
    if let Some((name, period)) = get_period_report_name(&symptom_onset_report)? {
        if let Some(trigger) = get_report_trigger(&symptom_onset_report)? {
            symptom_onset_report::init(context, name, period, trigger)?;
        }
        info!("Generating the symptom onset report.");
        report_count += 1;
    }
    let rt_name = get_report_name(&rt_report)?;
    let linelist_name = get_report_name(&offspring_distribution_report)?;
    let growth_reports_added = rt_name.is_some() as usize + linelist_name.is_some() as usize;
    if growth_reports_added > 0 {
        info!("Generating the growth rate report.");
        rt_report::init(context, rt_name, linelist_name)?;
        report_count += growth_reports_added;
    }
    info!("Generating {report_count} report(s) in total.");

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn disabled_writes_no_reports() {
        // Every field of `Reports` is a report; assert none of them write.
        let value = serde_json::to_value(Reports::disabled()).unwrap();
        for (name, report) in value.as_object().unwrap() {
            assert_eq!(
                report["write"],
                serde_json::json!(false),
                "{name} should be off"
            );
        }
    }
}
