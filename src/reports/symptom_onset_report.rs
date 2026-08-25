use crate::disease_manager::SymptomOnsetTime;
use crate::timekeeping::{ContextTimeKeeperExt, TimeKeeper};
use crate::{
    clinical_manager::CaseStatus, detection_manager::SurveillanceCampaignStartEvent,
    disease_manager::InfectionStatus, reports::ReportTrigger, shutdown::ShutdownEvent, PersonId,
};
use crate::{ContextParametersExt, ParameterValues, Person};
use anyhow::{bail, ensure, Result};
use ixa::{define_report, prelude::*, Context, ContextReportExt, HashMap};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct SymptomOnsetIncidenceReport {
    t_upper: f64,
    date: TimeKeeper,
    case_status: CaseStatus,
    count: usize,
}

define_report!(SymptomOnsetIncidenceReport);

// Preserve zero count values by intializing with a vec over all time periods up to `max_time`.
fn _get_zero_vec(period: f64, max_time: f64) -> Vec<usize> {
    vec![0; (max_time / period).ceil() as usize]
}

#[derive(Debug, PartialEq, Eq)]
enum OnsetFlag {
    InBounds, // Symptom onset time is within the max_time window of interest
    Overflow, // Symptom onset time is past the max_time window of interest
}

trait ContextSymptomReportExt: PluginContext + ContextParametersExt {
    /// Returns the report period index for a person based on their symptom onset time, or None if they have no onset time.
    /// Report periods are left-closed and right-open, so a person with symptom onset time `t` is included in the report period with index `i` where `period * i < t <= period * (i + 1)`.
    /// If a symptom onset time is past the `max_time` window of interest, the report period index is still returned, but the `OnsetFlag` will be set to `Overflow`.
    fn get_symptom_onset_report_period(
        &self,
        person_id: PersonId,
        max_time: f64,
    ) -> Result<Option<(usize, OnsetFlag)>> {
        let ParameterValues { reports, .. } = self.get_params();
        let period = reports.symptom_onset_report.clone().period;
        if let Some(onset_time) = self.get_property::<Person, SymptomOnsetTime>(person_id).0 {
            let time = onset_time.into_inner();
            if time < 0.0 {
                bail!("Person {person_id} has a negative symptom onset time {onset_time}");
            }
            if period.is_none() {
                bail!("Report period must be specified to calculate symptom onset report index");
            }
            let period = period.unwrap();

            // Valid to skip onset times that occur on or past max_time window of interest
            let index = (time / period).floor() as usize;
            if time < max_time {
                Ok(Some((index, OnsetFlag::InBounds)))
            } else {
                Ok(Some((index, OnsetFlag::Overflow)))
            }
        } else {
            Ok(None)
        }
    }
}
impl ContextSymptomReportExt for Context {}

/// Send symptom onset incidence reports for all cases with symptom onset times within the `max_time` window of interest, grouped by case status and report period.
/// If a case has a symptom onset time past the `max_time` window of interest, it will still be included in the report, but the report period vector will be resized to accommodate the overflow.
fn send_incidence_reports(context: &Context, period: f64, max_time: f64) -> Result<()> {
    let active_cases = context.query(with!(Person, InfectionStatus::Symptomatic));
    let removed_cases = context.query(with!(Person, InfectionStatus::Removed));
    let cases = active_cases.union(removed_cases);

    let mut case_status_onsets: HashMap<CaseStatus, Vec<usize>> = HashMap::default();

    for person_id in cases {
        let case_status = context.get_property::<Person, CaseStatus>(person_id);
        if let Some((report_period, onset_flag)) =
            context.get_symptom_onset_report_period(person_id, max_time)?
        {
            let onsets = case_status_onsets
                .entry(case_status)
                .or_insert(_get_zero_vec(period, max_time));
            if let OnsetFlag::Overflow = onset_flag {
                // onset was past max_time and the vec has to be extended
                let required_len = report_period + 1;
                if onsets.len() < required_len {
                    onsets.resize(required_len, 0);
                }
            }
            onsets[report_period] += 1;
        } else {
            bail!("Person {person_id} has no symptom onset time but did develop symptoms");
        }
    }

    for (case_status, binned_onsets) in case_status_onsets {
        let mut t_upper = period;

        // The first report period, times between 0.0 and `period`, are mapped to index 0
        for count in binned_onsets {
            context.send_report(SymptomOnsetIncidenceReport {
                t_upper,
                date: context.get_date_at_time(t_upper),
                case_status,
                count,
            });
            t_upper += period;
        }
    }
    Ok(())
}

pub fn init(
    context: &mut Context,
    file_name: &str,
    period: f64,
    trigger: ReportTrigger,
) -> Result<()> {
    let &ParameterValues {
        end_run_conditions, ..
    } = context.get_params();
    let max_time = end_run_conditions.max_time.into_inner();
    match trigger {
        ReportTrigger::Time { time } => {
            let time = time.into_inner();
            ensure!(
                time > 0.0,
                "Report time must be greater than 0.0, found {time}"
            );
            ensure!(
                time <= max_time,
                "Report time {time} is greater than the maximum simulation time {max_time}"
            );
            context.add_plan(time, move |context| {
                send_incidence_reports(context, period, time).unwrap();
            });
        }
        ReportTrigger::Shutdown => {
            context.subscribe_to_event::<ShutdownEvent>(move |context, event| {
                send_incidence_reports(context, period, event.time).unwrap();
            });
        }

        // If the outbreak ends before `max_time`, the `SurveillanceCampaign` trigger may not be called.
        ReportTrigger::SurveillanceCampaign { delay } => {
            context.subscribe_to_event::<SurveillanceCampaignStartEvent>(move |context, event| {
                let report_time = event.time + delay.into_inner();
                context.add_plan(report_time, move |context| {
                    send_incidence_reports(context, period, report_time).unwrap();
                });
            });
        }

        ReportTrigger::Date { date } => {
            let end_date = context.get_date_at_time(max_time);

            ensure!(date <= end_date, "Report date {date:?} is greater than the maximum simulation time {max_time} at date {end_date:?}");
            context.add_plan_from_timekeeper(date, move |context| {
                send_incidence_reports(context, period, max_time).unwrap();
            });
        }
    }

    context.add_report::<SymptomOnsetIncidenceReport>(file_name)?;
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::clinical_manager::CaseStatusValue;
    use crate::infection_initialization::{InfectionInitialization, NovelInfectionSource};
    use crate::parameters::{ContextParametersExt, ParameterValues, Parameters};
    use crate::reports::{ReportParams, Reports};
    use crate::shutdown::{plan_shutdown, EndRunConditions};
    use ixa::ContextGlobalPropertiesExt;
    use ordered_float::OrderedFloat;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn setup_context_with_report(symptom_onset_report: ReportParams, max_time: f64) -> Context {
        let mut context = Context::new();
        let end_run_conditions = EndRunConditions::from_max_time(max_time);
        context
            .set_global_property_value(
                Parameters,
                ParameterValues {
                    end_run_conditions,
                    initialization: InfectionInitialization {
                        start_date: TimeKeeper::new(2026, 1, 15).unwrap(),
                        initial_cases: NovelInfectionSource::None,
                    },
                    reports: Reports {
                        symptom_onset_report,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .unwrap();
        plan_shutdown(&mut context, end_run_conditions).unwrap();
        context
    }

    #[test]
    fn test_report_date_greater_than_max_time() {
        let mut context = Context::new();
        // Simulation ends 10 days after January first, so January 20th is past the max_time of the simulation
        let end_run_conditions = EndRunConditions::from_max_time(10.0);
        let report_date = TimeKeeper::new(2026, 1, 20).unwrap();
        context
            .set_global_property_value(
                Parameters,
                ParameterValues {
                    end_run_conditions,
                    reports: Reports {
                        symptom_onset_report: ReportParams {
                            write: true,
                            filename: Some("symptom_onset_report.csv".to_string()),
                            period: Some(1.0),
                            trigger: Some(ReportTrigger::Date { date: report_date }),
                        },
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .unwrap();
        let result = init(
            &mut context,
            "symptom_onset_report.csv",
            1.0,
            ReportTrigger::Date { date: report_date },
        );
        assert!(
            result.is_err(),
            "Initializing report with date trigger past max_time should return an error"
        );
    }

    #[test]
    fn test_get_symptom_onset_report_period() {
        let period = 2.0;
        let max_time = 10.0;
        let report_params = ReportParams {
            write: true,
            filename: Some("symptom_onset_report.csv".to_string()),
            period: Some(period),
            trigger: Some(ReportTrigger::Shutdown),
        };
        let mut context = setup_context_with_report(report_params, max_time);

        let examples = [
            (OrderedFloat(0.5), 0),
            (OrderedFloat(1.5), 0),
            (OrderedFloat(2.0), 1),
            (OrderedFloat(2.5), 1),
            (OrderedFloat(3.0), 1),
            (OrderedFloat(4.9), 2),
        ];

        for (onset_time, expected_period) in examples.iter() {
            let person_id = context
                .add_entity(with!(Person, SymptomOnsetTime(Some(*onset_time))))
                .unwrap();
            if let Some((report_period, OnsetFlag::InBounds)) = context
                .get_symptom_onset_report_period(person_id, max_time)
                .unwrap()
            {
                assert_eq!(
                    report_period, *expected_period,
                    "Symptom onset time of {:?} with period {:?} should be in report period {:?}",
                    onset_time, period, *expected_period
                );
            };
        }
    }

    #[test]
    fn test_resize_on_max_time() {
        let period = 1.0;
        let max_time = 3.0;
        let report_params = ReportParams {
            write: true,
            filename: Some("symptom_onset_report.csv".to_string()),
            period: Some(period),
            trigger: Some(ReportTrigger::Shutdown),
        };
        let mut context = setup_context_with_report(report_params, max_time);

        let person_id = context
            .add_entity(with!(Person, SymptomOnsetTime(Some(OrderedFloat(3.0)))))
            .unwrap();
        if let Some((report_period, onset_flag)) = context
            .get_symptom_onset_report_period(person_id, max_time)
            .unwrap()
        {
            assert_eq!(
                report_period, 3,
                "Symptom onset time of 3.0 with period {:?} should be in report period 3",
                period
            );
            assert_eq!(
                onset_flag,
                OnsetFlag::Overflow,
                "Symptom onset time of 3.0 should be marked as overflow"
            );
        };
    }

    #[test]
    #[should_panic]
    fn test_get_symptom_onset_report_period_invalid_time() {
        let mut context = setup_context_with_report(
            ReportParams {
                write: true,
                filename: Some("symptom_onset_report.csv".to_string()),
                period: Some(1.0),
                trigger: Some(ReportTrigger::Shutdown),
            },
            10.0,
        );
        let person_id = context
            .add_entity(with!(Person, SymptomOnsetTime(Some(OrderedFloat(-1.0)))))
            .unwrap();
        let _ = context
            .get_symptom_onset_report_period(person_id, 10.0)
            .unwrap();
    }

    #[test]
    fn test_onset_at_max_time_returns_overflow() {
        let mut context = setup_context_with_report(
            ReportParams {
                write: true,
                filename: Some("symptom_onset_report.csv".to_string()),
                period: Some(1.0),
                trigger: Some(ReportTrigger::Shutdown),
            },
            10.0,
        );
        let person_id = context
            .add_entity(with!(Person, SymptomOnsetTime(Some(OrderedFloat(10.0)))))
            .unwrap();
        let (report_period, onset_flag) = context
            .get_symptom_onset_report_period(person_id, 10.0)
            .unwrap()
            .unwrap();
        assert_eq!(
            report_period, 10,
            "Symptom onset time at max_time should return Some((10, OnsetFlag::Overflow))"
        );
        assert_eq!(
            onset_flag,
            OnsetFlag::Overflow,
            "Symptom onset time past max_time should return Some((10, OnsetFlag::Overflow))"
        );
    }

    #[test]
    fn test_get_onset_period_late() {
        let mut context = setup_context_with_report(
            ReportParams {
                write: true,
                filename: Some("symptom_onset_report.csv".to_string()),
                period: Some(1.0),
                trigger: Some(ReportTrigger::Shutdown),
            },
            5.0,
        );
        let person_id = context
            .add_entity(with!(Person, SymptomOnsetTime(Some(OrderedFloat(10.0)))))
            .unwrap();
        let (report_period, onset_flag) = context
            .get_symptom_onset_report_period(person_id, 5.0)
            .unwrap()
            .unwrap();
        assert_eq!(
            report_period, 10,
            "Symptom onset time past max_time should return Some((10, OnsetFlag::Overflow))"
        );
        assert_eq!(
            onset_flag,
            OnsetFlag::Overflow,
            "Symptom onset time past max_time should return Some((10, OnsetFlag::Overflow))"
        );
    }

    #[test]
    fn test_group_incidence_counts_on_shutdown() {
        let max_time = 3.0;
        let mut context = setup_context_with_report(
            ReportParams {
                write: true,
                filename: Some("symptom_onset_report.csv".to_string()),
                period: Some(1.0),
                trigger: Some(ReportTrigger::Shutdown),
            },
            max_time,
        );

        let temp_dir = tempdir().unwrap();
        let path = PathBuf::from(&temp_dir.path());
        let config = context.report_options();
        config.directory(path.clone());

        for _ in 0..5 {
            let _ = context
                .add_entity(with!(
                    Person,
                    CaseStatus(None),
                    InfectionStatus::Symptomatic,
                    SymptomOnsetTime(Some(OrderedFloat(0.5)))
                ))
                .unwrap();
        }
        for _ in 0..3 {
            let _ = context
                .add_entity(with!(
                    Person,
                    CaseStatus(Some(CaseStatusValue::Confirmed)),
                    InfectionStatus::Symptomatic,
                    SymptomOnsetTime(Some(OrderedFloat(2.5)))
                ))
                .unwrap();
        }
        for _ in 0..3 {
            let _ = context
                .add_entity(with!(
                    Person,
                    CaseStatus(None),
                    InfectionStatus::Symptomatic,
                    SymptomOnsetTime(Some(OrderedFloat(2.5)))
                ))
                .unwrap();
        }
        init(
            &mut context,
            "symptom_onset_report.csv",
            1.0,
            ReportTrigger::Shutdown,
        )
        .unwrap();
        context.execute();

        let symptom_onset_report = context.get_params().reports.symptom_onset_report.clone();
        let file_path = if let Some(name) = symptom_onset_report.filename {
            path.join(name)
        } else {
            panic!("No report name specified");
        };

        assert!(file_path.exists());
        std::mem::drop(context);

        assert!(file_path.exists());
        let mut reader = csv::Reader::from_path(file_path).unwrap();

        let mut actual: Vec<Vec<String>> = reader
            .records()
            .map(|result| result.unwrap().iter().map(String::from).collect())
            .collect();
        let mut expected = vec![
            //  [t_upper, date, case_status, count]
            vec!["1.0", "2026-01-16", "Confirmed", "0"],
            vec!["2.0", "2026-01-17", "Confirmed", "0"],
            vec!["3.0", "2026-01-18", "Confirmed", "3"],
            vec!["1.0", "2026-01-16", "", "5"],
            vec!["2.0", "2026-01-17", "", "0"],
            vec!["3.0", "2026-01-18", "", "3"],
        ];

        actual.sort();
        expected.sort();

        assert_eq!(actual, expected, "CSV file should contain the correct data");
    }
}
