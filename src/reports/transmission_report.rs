use anyhow::Result;
use ixa::{define_report, prelude::*, Context, ContextReportExt};
use serde::{Deserialize, Serialize};

use crate::detection_manager::TransmissionChain;
use crate::disease_manager::SymptomOnsetTime;
use crate::shutdown::ShutdownEvent;
use crate::timekeeping::{ContextTimeKeeperExt, TimeKeeper};
use crate::{Person, PersonId};

#[derive(Serialize, Deserialize, Copy, Clone)]
struct LineListReport {
    target_id: PersonId,
    infector_id: Option<PersonId>,
    infection_time: f64,
    symptom_onset_time: Option<f64>,
    infection_date: TimeKeeper,
    symptom_onset_date: Option<TimeKeeper>,
}

define_report!(LineListReport);

pub fn init(context: &mut Context, name: &str) -> Result<()> {
    context.add_report::<LineListReport>(name)?;
    context.subscribe_to_event(|context, _event: ShutdownEvent| {
        for person in context.get_entity_iterator::<Person>() {
            if let Some(data) = context.get_property::<Person, TransmissionChain>(person).0 {
                let infection_time = data.infection_time.into_inner();
                let symptom_onset_time = context
                    .get_property::<Person, SymptomOnsetTime>(person)
                    .0
                    .map(|t| t.into_inner());
                let report_entry = LineListReport {
                    target_id: person,
                    infector_id: data.infector_id,
                    infection_time,
                    symptom_onset_time,
                    infection_date: context.get_date_at_time(infection_time),
                    symptom_onset_date: symptom_onset_time.map(|t| context.get_date_at_time(t)),
                };
                context.send_report(report_entry);
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::detection_manager::TransmissionChainData;
    use crate::parameters::{ContextParametersExt, ParameterValues, Parameters};
    use crate::reports::{ReportParams, Reports};
    use crate::shutdown::{plan_shutdown, EndRunConditions};

    use std::path::PathBuf;
    use tempfile::tempdir;

    fn setup_context_with_report(transmission_chain_report: ReportParams) -> Context {
        let mut context = Context::new();
        let end_run_conditions = EndRunConditions::from_max_time(3.0);
        plan_shutdown(&mut context, end_run_conditions).unwrap();
        context
            .set_global_property_value(
                Parameters,
                ParameterValues {
                    end_run_conditions,
                    reports: Reports {
                        transmission_chain_report,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .unwrap();
        context.init_random(context.get_params().seed);
        context
    }

    #[test]
    fn test_line_list_report() {
        let mut context = setup_context_with_report(ReportParams {
            write: true,
            filename: Some("output.csv".to_string()),
            period: None,
            trigger: None,
        });

        let temp_dir = tempdir().unwrap();
        let path = PathBuf::from(&temp_dir.path());
        let config = context.report_options();
        config.directory(path.clone());

        // Generate linear transmission chain of 3 people with no symptom onset data
        let mut infector = None;
        for i in 0..3 {
            let person = if let Some(prev_infector) = infector {
                context
                    .add_entity(with!(
                        Person,
                        TransmissionChain(Some(TransmissionChainData::new_with_infector(
                            i as f64,
                            prev_infector,
                            Some(i - 1)
                        )))
                    ))
                    .unwrap()
            } else {
                context
                    .add_entity(with!(
                        Person,
                        TransmissionChain(Some(TransmissionChainData::new(0.0)))
                    ))
                    .unwrap()
            };
            infector = Some(person);
        }

        crate::reports::init(&mut context).unwrap();
        context.execute();

        let transmission_chain_report = context
            .get_params()
            .reports
            .transmission_chain_report
            .clone();
        let file_path = if let Some(name) = transmission_chain_report.filename {
            path.join(name)
        } else {
            panic!("No report name specified");
        };

        assert!(file_path.exists());
        std::mem::drop(context);
        let mut reader = csv::Reader::from_path(file_path).unwrap();

        let mut actual: Vec<Vec<String>> = reader
            .records()
            .map(|result| result.unwrap().iter().map(String::from).collect())
            .collect();
        let mut expected = vec![
            vec!["0", "", "0.0", "", "2025-01-01", ""],
            vec!["1", "0", "1.0", "", "2025-01-02", ""],
            vec!["2", "1", "2.0", "", "2025-01-03", ""],
        ];
        actual.sort();
        expected.sort();
        assert_eq!(actual, expected);
    }
}
