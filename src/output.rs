use std::fs::File;
use std::io::{BufWriter, Write};

use anyhow::Result;

use crate::types::EegPacket;

pub struct CsvWriter {
    writer: BufWriter<File>,
}

impl CsvWriter {
    pub fn create(path: &str) -> Result<Self> {
        let mut writer = BufWriter::new(File::create(path)?);
        writeln!(
            writer,
            "timestamp_us,counter,ref,drl,ch0,ch1,ch2,ch3,ch4,ch5,ch6,ch7,ch8,ch9,ch10,ch11,status"
        )?;
        Ok(Self { writer })
    }

    pub fn write_packet(&mut self, packet: &EegPacket) -> Result<()> {
        write!(
            self.writer,
            "{},{},{},{},",
            packet.timestamp_us, packet.counter, packet.ref_signal, packet.drl_signal
        )?;
        for idx in 0..packet.eeg_uv.len() {
            write!(self.writer, "{}", packet.eeg_uv[idx])?;
            if idx + 1 != packet.eeg_uv.len() {
                write!(self.writer, ",")?;
            }
        }
        writeln!(self.writer, ",{}", packet.status)?;
        Ok(())
    }
}
