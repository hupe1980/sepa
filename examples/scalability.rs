//! Labelled synthetic measurements, not participant acceptance or production SLAs.

use sepa::{DirectDebitEntry, DirectDebitGroup, IsoDate, Pain008Builder, ParseLimits};
use std::error::Error;
use std::fmt::Write as _;
use std::io::{self, Write};
use std::time::Instant;

const ENTRY: &str = "<Ntry><Amt Ccy=\"EUR\">0.01</Amt><CdtDbtInd>CRDT</CdtDbtInd><Sts><Cd>BOOK</Cd></Sts><BkTxCd><Prtry><Cd>SYNTHETIC</Cd></Prtry></BkTxCd></Ntry>";

fn reader_fixture(count: usize) -> Result<String, Box<dyn Error>> {
    let total = sepa::ct_to_eur_str(i64::try_from(count)?);
    let mut xml = String::from(
        "<Document xmlns=\"urn:iso:std:iso:20022:tech:xsd:camt.053.001.08\"><BkToCstmrStmt><GrpHdr><MsgId>SYNTHETIC</MsgId><CreDtTm>2026-10-12T12:00:00</CreDtTm></GrpHdr><Stmt><Id>SYNTHETIC</Id><CreDtTm>2026-10-12T12:00:00</CreDtTm><Acct><Id><IBAN>DE89370400440532013000</IBAN></Id><Ccy>EUR</Ccy></Acct><Bal><Tp><CdOrPrtry><Cd>OPBD</Cd></CdOrPrtry></Tp><Amt Ccy=\"EUR\">0.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><Dt><Dt>2026-10-12</Dt></Dt></Bal>",
    );
    write!(
        xml,
        "<Bal><Tp><CdOrPrtry><Cd>CLBD</Cd></CdOrPrtry></Tp><Amt Ccy=\"EUR\">{total}</Amt><CdtDbtInd>CRDT</CdtDbtInd><Dt><Dt>2026-10-12</Dt></Dt></Bal>"
    )?;
    for _ in 0..count {
        xml.push_str(ENTRY);
    }
    xml.push_str("</Stmt></BkToCstmrStmt></Document>");
    Ok(xml)
}

fn batch(count: usize) -> Result<Pain008Builder, Box<dyn Error>> {
    let creditor = sepa::validate_iban("DE89370400440532013000")?;
    let debtor = sepa::validate_iban("NL91ABNA0417164300")?;
    let creditor_id = sepa::validate_creditor_id("DE98ZZZ09999999999")?;
    let mandate_date = IsoDate::new(2026, 10, 1)?;
    let mut group = DirectDebitGroup::new(
        "Synthetic Creditor",
        &creditor,
        &creditor_id,
        IsoDate::new(2026, 10, 12)?,
    );
    for index in 0..count {
        group = group.add_entry(DirectDebitEntry::new(
            format!("MND-{index}"),
            mandate_date,
            "Synthetic Debtor",
            debtor.clone(),
            1,
            format!("TX-{index}"),
        ));
    }
    Ok(Pain008Builder::new("Synthetic Creditor", "SYNTHETIC-BATCH")
        .created_at("2026-10-12T12:00:00".parse()?)
        .add_group(group))
}

#[derive(Debug, Default)]
struct CountingWriter {
    bytes: usize,
}

impl Write for CountingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(bytes.len())
            .ok_or_else(|| io::Error::other("byte count overflow"))?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn measure_reader(count: usize, large: bool) -> Result<(), Box<dyn Error>> {
    let xml = reader_fixture(count)?;
    let limits = if large {
        ParseLimits {
            nodes: count
                .checked_mul(8)
                .and_then(|nodes| nodes.checked_add(64))
                .ok_or("node budget overflow")?,
            ..Default::default()
        }
    } else {
        ParseLimits::default()
    };
    let started = Instant::now();
    let parsed = sepa::parse_camt053_with_limits(&xml, limits);
    let elapsed_ns = started.elapsed().as_nanos();
    match parsed {
        Ok(document) => {
            let statement = document.statements.first().ok_or("missing statement")?;
            let expected = i64::try_from(count)?;
            if document.statements.len() != 1
                || statement.entries.len() != count
                || statement.net_movement_ct() != Some(expected)
                || statement
                    .closing_balance()
                    .and_then(sepa::StatementBalance::signed_ct)
                    != Some(expected)
            {
                return Err("synthetic reader fact mismatch".into());
            }
            std::hint::black_box(&document);
            println!(
                "{{\"mode\":\"reader\",\"count\":{count},\"bytes\":{},\"nodes_limit\":{},\"status\":\"passed\",\"elapsed_ns\":{elapsed_ns}}}",
                xml.len(),
                limits.nodes
            );
        }
        Err(sepa::Camt053ParseError::Xml(sepa::XmlError::LimitExceeded {
            resource,
            limit,
            actual,
        })) => {
            println!(
                "{{\"mode\":\"reader\",\"count\":{count},\"bytes\":{},\"nodes_limit\":{},\"status\":\"refused\",\"resource\":\"{resource}\",\"limit\":{limit},\"actual\":{actual},\"elapsed_ns\":{elapsed_ns}}}",
                xml.len(),
                limits.nodes
            );
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn measure_writer(mode: &str, count: usize) -> Result<(), Box<dyn Error>> {
    let builder = batch(count)?;
    let started = Instant::now();
    let (bytes, elapsed_ns) = if mode == "build" {
        let xml = builder.build()?;
        let elapsed_ns = started.elapsed().as_nanos();
        if xml.matches("<DrctDbtTxInf>").count() != count
            || !xml.contains(&format!(
                "<CtrlSum>{}</CtrlSum>",
                sepa::ct_to_eur_str(i64::try_from(count)?)
            ))
        {
            return Err("synthetic writer fact mismatch".into());
        }
        (xml.len(), elapsed_ns)
    } else {
        let mut output = CountingWriter::default();
        builder.write_to(&mut output)?;
        (output.bytes, started.elapsed().as_nanos())
    };
    if bytes == 0 {
        return Err("empty writer output".into());
    }
    println!(
        "{{\"mode\":\"{mode}\",\"count\":{count},\"bytes\":{bytes},\"status\":\"passed\",\"elapsed_ns\":{elapsed_ns}}}"
    );
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = std::env::args().skip(1);
    let mode = arguments
        .next()
        .ok_or("mode required: reader, build, stream, reader-xml, build-xml, stream-xml")?;
    let count: usize = arguments
        .next()
        .ok_or("count required: 1000, 10000, 100000")?
        .parse()?;
    if ![1000, 10_000, 100_000].contains(&count) {
        return Err("count must be 1000, 10000 or 100000".into());
    }
    let policy = arguments.next().unwrap_or_else(|| "default".to_owned());
    if !["default", "large"].contains(&policy.as_str()) || arguments.next().is_some() {
        return Err("only default or explicit large policy is accepted".into());
    }
    match mode.as_str() {
        "reader" => measure_reader(count, policy == "large")?,
        "build" | "stream" => measure_writer(&mode, count)?,
        "reader-xml" => io::stdout()
            .lock()
            .write_all(reader_fixture(count)?.as_bytes())?,
        "build-xml" => io::stdout()
            .lock()
            .write_all(batch(count)?.build()?.as_bytes())?,
        "stream-xml" => batch(count)?.write_to(&mut io::stdout().lock())?,
        _ => return Err("unknown measurement mode".into()),
    }
    Ok(())
}
