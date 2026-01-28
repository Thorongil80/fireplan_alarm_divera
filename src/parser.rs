use crate::{Configuration, ParsedData, Ric, SubmitPayload};
use anyhow::Result;
use log::{error, warn};
use regex::Regex;

pub fn parse(
    data: SubmitPayload,
    configuration: Configuration,
) -> Result<ParsedData> {
    let mut result = ParsedData {
        rics: vec![],
        einsatznrlst: "".to_string(),
        strasse: "".to_string(),
        hausnummer: "".to_string(),
        ort: "".to_string(),
        ortsteil: "".to_string(),
        objektname: "".to_string(),
        koordinaten: "".to_string(),
        einsatzstichwort: "".to_string(),
        zusatzinfo: "".to_string(),
    };

    return Ok(result);  // do nothing below, need to disable parser for testing

    // remove creepy windows line endings
    let body = data.text.replace('\r', "");

    for line in body.lines() {

        if let Ok(re) = Regex::new(configuration.regex_strasse.as_str()) {
            if let Some(caps) = re.captures(line) {
                result.strasse = caps[1].to_string();
            }
        } else {
            error!(
                "regex_strasse is not a proper regular expression",
            );
        }

        if let Ok(re) = Regex::new(configuration.regex_hausnummer.as_str()) {
            if let Some(caps) = re.captures(line) {
                result.hausnummer = caps[1].to_string();
            }
        } else {
            error!(
                "regex_hausnummer is not a proper regular expression",
            );
        }

        if let Ok(re) = Regex::new(configuration.regex_ort.as_str()) {
            if let Some(caps) = re.captures(line) {
                result.ort = caps[1].to_string();
            }
        } else {
            error!(
                "regex_ort is not a proper regular expression",
            );
        }

        if let Ok(re) = Regex::new(configuration.regex_ortsteil.as_str()) {
            if let Some(caps) = re.captures(line) {
                result.ortsteil = caps[1].to_string();
            }
        } else {
            error!(
                "regex_ortsteil is not a proper regular expression",
            );
        }

        if let Ok(re) = Regex::new(configuration.regex_koordinaten.as_str()) {
            if let Some(caps) = re.captures(line) {
                result.koordinaten = caps[1].to_string();
            }
        } else {
            error!("regex_koordinaten is not a proper regular expression",
            );
        }

        if let Ok(re) = Regex::new(configuration.regex_objektname.as_str()) {
            if let Some(caps) = re.captures(line) {
                result.objektname = caps[1].to_string();
            }
        } else {
            error!(
                "regex_objektname is not a proper regular expression",
            );
        }
    }

    if let Ok(re) = Regex::new(configuration.regex_zusatzinfo.as_str()) {
        if let Some(caps) = re.captures(body.as_str()) {
            result.zusatzinfo = caps[1].to_string();
        }
    } else {
        error!(
            "regex_zusatzinfo is not a proper regular expression",
        );
    }

    for line in body.lines() {
        // detect rics by text
        let mut temp_lines: Vec<Ric> = vec![];
        for ric in configuration.rics.clone() {
            if line.contains(ric.text.as_str()) {
                // remove all previously found entries that are substrings, retain what is not a substring of the newly found
                temp_lines.retain(|x| !ric.text.contains(x.clone().text.as_str()));

                let new_ric = Ric {
                    text: ric.text.clone(),
                    ric: format!("{:0>7}", ric.ric),
                    subric: ric.subric.clone(),
                };

                temp_lines.push(new_ric);
            }
        }
        result.rics.append(&mut temp_lines);
    }

    result.einsatzstichwort = data.title;
    result.ortsteil = result.ortsteil.trim().to_string();
    result.objektname = result.objektname.trim().to_string();
    result.ort = result.ort.trim().to_string();
    result.einsatznrlst = data.foreign_id;
    result.einsatzstichwort = result.einsatzstichwort.trim().to_string();
    result.strasse = result.strasse.trim().to_string();
    result.hausnummer = result.hausnummer.trim().to_string();
    result.zusatzinfo = result.zusatzinfo.trim().to_string();

    if result.einsatzstichwort.is_empty() {
        warn!("Parser: No EINSATZSTICHWORT found");
    }
    if result.ortsteil.is_empty() {
        warn!("Parser: No ORTSTEIL found");
    }
    if result.objektname.is_empty() {
        warn!("Parser: No OBJEKTNAME found");
    }
    if result.ort.is_empty() {
        warn!("Parser: No ORT found");
    }
    if result.einsatznrlst.is_empty() {
        warn!("Parser: No EINSATZNUMMERLEITSTELLE found");
    }
    if result.einsatzstichwort.is_empty() {
        warn!("Parser: No EINSATZSTICHWORT found");
    }
    if result.strasse.is_empty() {
        warn!("Parser: No STRASSE found");
    }
    if result.hausnummer.is_empty() {
        warn!("Parser: No HAUSNUMMER found");
    }

    Ok(result)
}
