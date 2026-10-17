//! Mission conversations (`<map>.conv`): the Italian dialogue lines of a
//! scenario with their speakers and voice files.
//!
//! Layout, little endian, strings zero-terminated in Windows-1252:
//! u16 conversations; each one: name, u16 phrases; each phrase: u16 index
//! (from 1), u16 speaker (object id), u8 start flag, name, condition, action, an
//! always-empty string, voice file, text, u16 link count and that many u16 indices
//! of the phrases that may follow.
use crate::{Result, assets::legacy_text, invalid};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Phrase {
    pub index: u16,
    /// Id of the map object that speaks (a hero or a unit of the scenario);
    /// 0 for narration.
    pub speaker: u16,
    pub start: bool,
    /// Condition on mission variables (`$name==value`), usually empty.
    pub condition: String,
    /// Script command run with the line (`$name=value`), usually empty.
    pub action: String,
    pub voice: String,
    pub text: String,
    pub next: Vec<u16>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Conversation {
    pub name: String,
    pub phrases: Vec<Phrase>,
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}
impl Reader<'_> {
    fn bytes(&mut self, n: usize) -> Result<&[u8]> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.data.len());
        let end = end.ok_or_else(|| invalid("truncated conversation file"))?;
        let s = &self.data[self.at..end];
        self.at = end;
        Ok(s)
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into()?))
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }
    fn string(&mut self) -> Result<String> {
        let rest = &self.data[self.at..];
        let len = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| invalid("unterminated conversation text"))?;
        let text = legacy_text(&rest[..len]);
        self.at += len + 1;
        Ok(text)
    }
}

pub fn parse(data: &[u8]) -> Result<Vec<Conversation>> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let mut r = Reader { data, at: 0 };
    let count = r.u16()?;
    let mut conversations = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let name = r.string()?;
        let phrases = r.u16()?;
        let mut list = Vec::with_capacity(phrases as usize);
        for _ in 0..phrases {
            let index = r.u16()?;
            let speaker = r.u16()?;
            let start = r.u8()? != 0;
            let _name = r.string()?;
            // Mission-variable script: `$lena==2` gates the line, `$presto=1`
            // sets a variable. A third string is empty in every original file.
            let condition = r.string()?;
            let action = r.string()?;
            let _unused = r.string()?;
            let voice = r.string()?;
            let text = r.string()?.trim().replace("\r\n", "\n");
            let links = r.u16()?;
            if links > phrases {
                return Err(invalid(format!(
                    "conversation «{name}» phrase {index}: {links} links at byte {} for {phrases} phrases",
                    r.at - 2
                )));
            }
            let next = (0..links).map(|_| r.u16()).collect::<Result<Vec<_>>>()?;
            list.push(Phrase {
                index,
                speaker,
                start,
                condition,
                action,
                voice,
                text,
                next,
            });
        }
        conversations.push(Conversation {
            name,
            phrases: list,
        });
    }
    if r.at != data.len() {
        return Err(invalid(format!(
            "{} bytes left after the conversations",
            data.len() - r.at
        )));
    }
    Ok(conversations)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn phrases_links_and_italian_text() {
        let mut d = vec![1, 0];
        d.extend_from_slice(b"Conversacion0001\0");
        d.extend_from_slice(&[2, 0]);
        // Phrase 1: speaker 828, start, links to phrase 2.
        d.extend_from_slice(&[1, 0, 0x3c, 0x03, 1]);
        d.extend_from_slice(
            b"Frase0001\0$lena==2\0$presto=1\0\0USER\\F01.wav\0Art\xf9 \xe8 qui.\0",
        );
        d.extend_from_slice(&[1, 0, 2, 0]);
        // Phrase 2: last one, no links.
        d.extend_from_slice(&[2, 0, 0x3d, 0x03, 0]);
        d.extend_from_slice(b"Frase0002\0\0\0\0USER\\F02.wav\0Torniamo a casa...\r\n\0");
        d.extend_from_slice(&[0, 0]);
        let c = parse(&d).unwrap();
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].phrases[0].text, "Artù è qui.");
        assert_eq!(c[0].phrases[0].speaker, 828);
        assert_eq!(c[0].phrases[0].condition, "$lena==2");
        assert_eq!(c[0].phrases[0].action, "$presto=1");
        assert!(c[0].phrases[1].action.is_empty());
        assert!(c[0].phrases[0].start && !c[0].phrases[1].start);
        assert_eq!(c[0].phrases[0].next, [2]);
        assert_eq!(c[0].phrases[1].text, "Torniamo a casa...");
        assert!(parse(&d[..d.len() - 1]).is_err());
        assert!(parse(&[]).unwrap().is_empty());
    }
}
