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

    // remove creepy windows line endings
    let body = data.text.replace('\r', "");

    for line in body.lines() {



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

    // detect rics by text - now only in the substring after "Einsatzmittel:"
    let rics_source = if let Some(start) = body.find("Einsatzmittel:") {
        let start_idx = start + "Einsatzmittel:".len();
        body[start_idx..].to_string()
    } else {
        String::new()
    };

    let abt1_dummy_ric = Ric {
        text: "Dummy Abt 1".to_string(),
        ric: "0999991".to_string(),
        subric: "B".to_string(),
    };

    let abt2_dummy_ric = Ric {
        text: "Dummy Abt 2".to_string(),
        ric: "0999992".to_string(),
        subric: "B".to_string(),
    };

    let abt3_dummy_ric = Ric {
        text: "Dummy Abt 3".to_string(),
        ric: "0999993".to_string(),
        subric: "B".to_string(),
    };

    let abt4_dummy_ric = Ric {
        text: "Dummy Abt 4".to_string(),
        ric: "0999994".to_string(),
        subric: "B".to_string(),
    };

    for token in rics_source.split(',') {
        let mut temp_lines: Vec<Ric> = vec![];
        for ric in configuration.rics.clone() {
            if token.contains(ric.text.as_str()) {
                // remove all previously found entries that are substrings, retain what is not a substring of the newly found
                // each comma-separated part contains at maximum one RIC, so this is safe
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

    // always add KdoW RIC
    let kdow_dummy_ric = Ric {
        text: "Dummy KdoW".to_string(),
        ric: "0999995".to_string(),
        subric: "B".to_string(),
    };

    result.rics.push(kdow_dummy_ric);

    // loop tokens again to check for vehicle names
    for token in rics_source.split(',') {
        if token.contains("UW 1/") {
            if ! result.rics.contains(&abt1_dummy_ric) {
                result.rics.push(abt1_dummy_ric.clone());
            }
        }

        if token.contains("UW 2/") {
            if ! result.rics.contains(&abt2_dummy_ric) {
                result.rics.push(abt2_dummy_ric.clone());
            }
        }

        if token.contains("UW 3/") {
            if ! result.rics.contains(&abt3_dummy_ric) {
                result.rics.push(abt3_dummy_ric.clone());
            }
        }

        if token.contains("UW 4/") {
            if ! result.rics.contains(&abt4_dummy_ric) {
                result.rics.push(abt4_dummy_ric.clone());
            }
        }
    }

    // trim spaces from all string fields
    result.einsatzstichwort = data.title.trim().to_string();
    result.ortsteil = result.ortsteil.trim().to_string();
    result.objektname = result.objektname.trim().to_string();
    result.ort = result.ort.trim().to_string();
    result.einsatznrlst = data.foreign_id;

    // Google Maps coordinates from lat/lng (format: "lat,lng")
    result.koordinaten = format!("{},{}", data.lat.trim(), data.lng.trim());

    // on the left hand-side of the first comma is the street name
    result.strasse = data.address.split(',').next().unwrap_or("").split_whitespace().next().unwrap_or("").to_string();

    // on the right hand-side of the first space in the strasse element is the house number (if any)
    result.hausnummer = data
        .address
        .split(',')
        .next()
        .unwrap_or("")
        .split_whitespace()
        .nth(1)
        .unwrap_or("")
        .to_string();

    // extract zusatzinfo between "Meldung:" and "Schlagwort" from the original text
    if let Some(start_idx) = data.text.find("Meldung:") {
        let after_start = start_idx + "Meldung:".len();
        if let Some(end_idx_rel) = data.text[after_start..].find("Schlagwort") {
            let end_idx = after_start + end_idx_rel;
            result.zusatzinfo = data.text[after_start..end_idx].trim().to_string();
        } else {
            // no end marker found -> empty
            result.zusatzinfo = String::new();
        }
    } else {
        // no start marker found -> empty
        result.zusatzinfo = String::new();
    }

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
