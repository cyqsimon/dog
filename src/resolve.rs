//! Specifying the address of the DNS server to send requests to.

use std::io;

use log::*;

use dns::Labels;


/// A **resolver type** is the source of a `Resolver`.
#[derive(PartialEq, Debug)]
pub enum ResolverType {

    /// Obtain a resolver by consulting the system in order to find a
    /// nameserver and a search list.
    SystemDefault,

    /// Obtain a resolver by using the given user-submitted string.
    Specific(String),
}

impl ResolverType {

    /// Obtains a resolver by the means specified in this type. Returns an
    /// error if there was a problem looking up system information.
    pub fn obtain(self) -> io::Result<Resolver> {
        match self {
            Self::SystemDefault => {
                system_nameservers()
            }
            Self::Specific(nameserver) => {
                let search_list = Vec::new();
                Ok(Resolver { nameserver, search_list })
            }
        }
    }
}


/// A **resolver** knows the address of the server we should
/// send DNS requests to, and the search list for name lookup.
#[derive(Debug)]
pub struct Resolver {

    /// The address of the nameserver.
    pub nameserver: String,

    /// The search list for name lookup.
    pub search_list: Vec<String>,
}

impl Resolver {

    /// Returns a nameserver that queries should be sent to.
    pub fn nameserver(&self) -> String {
        self.nameserver.clone()
    }

    /// Returns a sequence of names to be queried, taking into account
    /// the search list.
    pub fn name_list(&self, name: &Labels) -> Vec<Labels> {
        let mut list = Vec::new();

        if name.len() > 1 {
            list.push(name.clone());
            return list;
        }

        for search in &self.search_list {
            match Labels::encode(search) {
                Ok(suffix)  => list.push(name.extend(&suffix)),
                Err(_)      => panic!("Invalid search list {}", search),
            }
        }

        list.push(name.clone());
        list
    }
}


/// Looks up the system default nameserver on Unix, by querying
/// `/etc/resolv.conf` and using the first line that specifies one.
/// Returns an error if there’s a problem reading the file, or `None` if no
/// nameserver is specified in the file.
#[cfg(unix)]
fn system_nameservers() -> io::Result<Resolver> {
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    if cfg!(test) {
        panic!("system_nameservers() called from test code");
    }

    let f = File::open("/etc/resolv.conf")?;
    let reader = BufReader::new(f);

    let mut nameservers = Vec::new();
    let mut search_list = Vec::new();
    for line in reader.lines() {
        let line = line?;

        if let Some(nameserver_str) = line.strip_prefix("nameserver ") {
            let ip: Result<std::net::Ipv4Addr, _> = nameserver_str.parse();
            // TODO: This will need to be changed for IPv6 support.

            match ip {
                Ok(_ip) => nameservers.push(nameserver_str.into()),
                Err(e)  => warn!("Failed to parse nameserver line {:?}: {}", line, e),
            }
        }

        if let Some(search_str) = line.strip_prefix("search ") {
            search_list.clear();
            search_list.extend(search_str.split_ascii_whitespace().map(|s| s.into()));
        }
    }

    let nameserver = nameservers.into_iter().next().unwrap();
    Ok(Resolver { nameserver, search_list })
}


/// Looks up the system default nameserver on Windows, by iterating through
/// the list of network adapters and returning the first nameserver it finds.
#[cfg(windows)]
#[allow(unused)]  // todo: Remove this when the time is right
fn system_nameservers() -> io::Result<Resolver> {
    use std::net::{IpAddr, UdpSocket};

    if cfg!(test) {
        panic!("system_nameservers() called from test code");
    }

    let adapters = match ipconfig::get_adapters() {
        Ok(a) => a,
        Err(e) => {
            warn!("Error getting network adapters: {}", e);
            return Ok((None, Vec::new()));
        }
    };

    // According to the specification, prefer ipv6 by default.
    // TODO: add control flag to select an ip family.
    #[derive(Debug, PartialEq)]
    enum ForceIPFamily {
        V4,
        V6,
        None,
    }

    // get the IP of the Network adapter that is used to access the Internet
    // https://stackoverflow.com/questions/24661022/getting-ip-adress-associated-to-real-hardware-ethernet-controller-in-windows-c
    fn get_ipv4() -> io::Result<IpAddr> {
        let s = UdpSocket::bind("0.0.0.0:0")?;
        s.connect("8.8.8.8:53")?;
        let addr = s.local_addr()?;
        Ok(addr.ip())
    }

    fn get_ipv6() -> io::Result<IpAddr> {
        let s = UdpSocket::bind("[::1]:0")?;
        s.connect("[2001:4860:4860::8888]:53")?;
        let addr = s.local_addr()?;
        Ok(addr.ip())
    }

    let force_ip_family: ForceIPFamily = ForceIPFamily::None;
    let ip = match force_ip_family {
        ForceIPFamily::V4 => get_ipv4().ok(),
        ForceIPFamily::V6 => get_ipv6().ok(),
        ForceIPFamily::None => get_ipv6().or(get_ipv4()).ok(),
    };

    let search_path = Vec::new();  // todo: implement this

    let active_adapters = adapters.iter().filter(|a| {
        a.oper_status() == ipconfig::OperStatus::IfOperStatusUp && !a.gateways().is_empty()
    });

    if let Some(dns_server) = active_adapters
        .clone()
        .find(|a| ip.map(|ip| a.ip_addresses().contains(&ip)).unwrap_or(false))
        .map(|a| a.dns_servers().first())
        .flatten()
    {
        debug!("Found first nameserver {:?}", dns_server);
        let nameserver = dns_server.to_string();
        Ok(Resolver { nameserver, search_path })
    }

    // Fallback
    else if let Some(dns_server) = active_adapters
        .flat_map(|a| a.dns_servers())
        .find(|d| (d.is_ipv4() && force_ip_family != ForceIPFamily::V6) || d.is_ipv6())
    {
        debug!("Found first fallback nameserver {:?}", dns_server);
        let nameserver = dns_server.to_string();
        Ok(Resolver { nameserver, search_path })
    }

    else {
        panic!("No nameservers available");
    }
}


/// The fall-back system default nameserver determinator that is not very
/// determined as it returns nothing without actually checking anything.
#[cfg(all(not(unix), not(windows)))]
fn system_nameservers() -> io::Result<(Option<String>, Vec<String>)> {
    warn!("Unable to fetch default nameservers on this platform.");
    Ok((None, Vec::new()))
}
