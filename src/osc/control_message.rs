use anyhow::{bail, ensure};
use rosc::{OscMessage, OscType};

use super::{OscClientId, OscError};

/// Wrapper type for OSC messages that provides a simplification for our domain.
/// This includes pre-processing of the address to identify the breaks, as well
/// as parsing of the group ID.
#[derive(Debug)]
pub struct OscControlMessage {
    /// The ID of the client that originated this message.
    pub client_id: OscClientId,
    /// The raw/full OSC address.
    addr: String,
    /// Single OSC payload extracted from the incoming message.
    pub arg: OscType,
    addr_index: AddressIndex,
}

#[derive(Debug)]
struct AddressIndex {
    /// The byte index in the addr string of the first character of the control
    /// portion of the address, including the leading slash.
    control_start: usize,
    /// The byte index in the addr string of the first character after the
    /// control key. For addrs with no payload following the control key,
    /// this may be equal to the length of the address and thus we must be
    /// careful not to accidentally try to slice past the end of the address.
    control_end: usize,
}

impl OscControlMessage {
    pub fn new(msg: OscMessage, client_id: OscClientId) -> Result<Self, OscError> {
        let wrap_err = |m: anyhow::Error| OscError {
            addr: msg.addr.clone(),
            msg: m.to_string(),
        };

        let addr_index = parse_address(&msg.addr).map_err(wrap_err)?;
        let arg = get_single_arg(msg.args).map_err(wrap_err)?;

        Ok(Self {
            client_id,
            addr: msg.addr,
            arg,
            addr_index,
        })
    }

    /// Return the first half of the control key, excluding the leading slash.
    pub fn group(&self) -> &str {
        &self.addr[1..self.addr_index.control_start]
    }

    /// Return the control portion of the address.
    pub fn control(&self) -> &str {
        &self.addr[self.addr_index.control_start + 1..self.addr_index.control_end]
    }

    /// Return the portion of the address following the control key.
    /// This will include a leading / if not empty.
    pub fn addr_payload(&self) -> &str {
        if self.addr_index.control_end == self.addr.len() {
            return "";
        }
        &self.addr[self.addr_index.control_end..]
    }

    /// Generate an OscError.
    pub fn err<M: Into<String>>(&self, msg: M) -> OscError {
        OscError {
            addr: self.addr.to_string(),
            msg: msg.into(),
        }
    }
}

fn parse_address(addr: &str) -> anyhow::Result<AddressIndex> {
    let mut slash_iter = addr
        .char_indices()
        .filter_map(|(i, c)| (c == '/').then_some(i));
    ensure!(
        slash_iter.next() == Some(0),
        "OSC address did not start with a slash"
    );

    let Some(control_start) = slash_iter.next() else {
        bail!("OSC address only had one path component");
    };

    ensure!(control_start > 1, "OSC address has empty group");

    let control_end = slash_iter.next().unwrap_or(addr.len());

    ensure!(
        control_end > control_start + 1,
        "OSC address has empty control"
    );

    Ok(AddressIndex {
        control_start,
        control_end,
    })
}

fn get_single_arg(mut args: Vec<OscType>) -> anyhow::Result<OscType> {
    ensure!(
        args.len() == 1,
        "message has {} args (expected one)",
        args.len()
    );
    Ok(args.pop().unwrap())
}

#[cfg(test)]
mod test {
    use std::{net::SocketAddr, str::FromStr};

    use super::*;
    use rosc::OscType;
    #[test]
    fn test_get_control_key() {
        assert_eq!(
            ("foo".to_string(), "bar".to_string()),
            get_control_key("/foo/bar/baz").unwrap()
        );
        assert_eq!(
            ("foo".to_string(), "bar".to_string()),
            get_control_key("/foo/bar").unwrap()
        );
        let bad = ["", "foo", "foo/bar", "/bar", "/", "//", "/f//"];
        for b in bad.iter() {
            assert!(get_control_key(b).is_err());
        }
    }

    fn get_control_key(addr: &str) -> Result<(String, String), OscError> {
        let msg = OscControlMessage::new(
            OscMessage {
                addr: addr.to_string(),
                args: vec![OscType::Nil],
            },
            OscClientId(SocketAddr::from_str("127.0.0.1:1234").unwrap()),
        )?;
        Ok((msg.group().to_string(), msg.control().to_string()))
    }
}
