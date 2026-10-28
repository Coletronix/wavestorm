//! Web requests for placing a site and fetching its soils: address lookup, map images and
//! NRCS Soil Data Access. Requests run in the background (a thread on the desktop, `fetch` in
//! the browser); replies arrive through an inbox polled each frame, like picked files. Nothing
//! goes online unless the user asks for it.

use std::sync::{Arc, Mutex, PoisonError};
use wavestorm_core::geo::{self, LonLat, MapSource};
use wavestorm_core::soils;

/// What a request is for.
#[derive(Debug, Clone, PartialEq)]
pub enum Fetch {
    /// Find an address.
    Geocode(String),
    /// A north-up map image of the site.
    Map { source: MapSource, center: LonLat, width_ft: f64, height_ft: f64, place: String },
    /// Soil map units clipped to an area of interest (longitude/latitude).
    Soils { aoi: Vec<LonLat>, drained: bool },
    /// NOAA Atlas 14 precipitation frequency depths at a point.
    Noaa { lon_lat: LonLat },
}

impl Fetch {
    /// What the status bar says while it runs.
    pub fn busy_text(&self) -> &'static str {
        match self {
            Fetch::Geocode(_) => "Looking up the address...",
            Fetch::Map { .. } => "Downloading the map image from USGS The National Map...",
            Fetch::Soils { .. } => "Asking NRCS Soil Data Access for the soils of the area of interest...",
            Fetch::Noaa { .. } => "Asking NOAA for the site's precipitation frequency estimates...",
        }
    }
}

/// A finished request: the body, or why there is none.
#[derive(Debug, Clone)]
pub struct Reply {
    pub fetch: Fetch,
    pub result: Result<Vec<u8>, String>,
}

#[derive(Clone)]
pub struct Net {
    inbox: Arc<Mutex<Vec<Reply>>>,
    pending: Arc<Mutex<usize>>,
    /// Requests are sent (off for headless runs: tests push replies themselves).
    online: bool,
}

impl Net {
    pub fn new(online: bool) -> Self {
        Net { inbox: Arc::default(), pending: Arc::default(), online }
    }

    pub fn take(&self) -> Vec<Reply> {
        std::mem::take(&mut *self.inbox.lock().unwrap_or_else(PoisonError::into_inner))
    }

    pub fn push(&self, reply: Reply) {
        self.inbox.lock().unwrap_or_else(PoisonError::into_inner).push(reply);
    }

    /// Requests still running.
    pub fn busy(&self) -> bool {
        *self.pending.lock().unwrap_or_else(PoisonError::into_inner) > 0
    }

    /// Starts a request; its reply arrives through [`Net::take`].
    pub fn start(&self, ctx: &egui::Context, fetch: Fetch) {
        let request = match build(&fetch) {
            Ok(r) => r,
            Err(e) => return self.push(Reply { fetch, result: Err(e) }),
        };
        if !self.online {
            return self.push(Reply { fetch, result: Err("Not connected (automated run).".into()) });
        }
        *self.pending.lock().unwrap_or_else(PoisonError::into_inner) += 1;
        let (inbox, pending, ctx) = (self.inbox.clone(), self.pending.clone(), ctx.clone());
        ehttp::fetch(request, move |r| {
            let result = match r {
                Ok(resp) => check(&fetch, resp),
                Err(e) => Err(format!("No connection to the service ({e}). Check the internet connection.")),
            };
            inbox.lock().unwrap_or_else(PoisonError::into_inner).push(Reply { fetch, result });
            let mut n = pending.lock().unwrap_or_else(PoisonError::into_inner);
            *n = n.saturating_sub(1);
            ctx.request_repaint();
        });
    }
}

fn build(fetch: &Fetch) -> Result<ehttp::Request, String> {
    let req = match fetch {
        Fetch::Geocode(q) => ehttp::Request::get(geo::geocode_url(q)),
        Fetch::Map { source, center, width_ft, height_ft, .. } => {
            ehttp::Request::get(geo::map_request(*source, *center, *width_ft, *height_ft)?.url)
        }
        Fetch::Soils { aoi, .. } => {
            let body = soils::sda_request(aoi)?;
            // Replace (not add to) the default text/plain content type: Soil Data Access's
            // server rejects a request with two.
            ehttp::Request::post(soils::SDA_URL, body.into_bytes())
                .with_headers(ehttp::Headers::new(&[("Accept", "*/*"), ("Content-Type", "application/json")]))
        }
        Fetch::Noaa { lon_lat } => {
            // The NOAA server does not allow web pages to read its answers (no CORS header).
            if cfg!(target_arch = "wasm32") {
                return Err("NOAA's server cannot be reached from the browser version. Download the CSV from the \
                            Precipitation Frequency Data Server (hdsc.nws.noaa.gov) and use Load File..."
                    .into());
            }
            ehttp::Request::get(wavestorm_core::noaa::pfds_url(lon_lat[1], lon_lat[0], true))
        }
    };
    // Nominatim's usage policy asks applications to identify themselves (browsers send their
    // own User-Agent and refuse to change it).
    #[cfg(not(target_arch = "wasm32"))]
    let req = req.with_header(
        "User-Agent",
        concat!("WAVEStorm/", env!("CARGO_PKG_VERSION"), " (+https://github.com/Coletronix/wavestorm)"),
    );
    Ok(req.with_timeout(Some(std::time::Duration::from_secs(120))))
}

/// The body of a reply the request can use.
fn check(fetch: &Fetch, resp: ehttp::Response) -> Result<Vec<u8>, String> {
    match fetch {
        // Soil Data Access reports query errors with status 400 and an explanation.
        Fetch::Soils { .. } if resp.status == 400 || resp.ok => Ok(resp.bytes),
        Fetch::Map { .. } if resp.ok && !resp.content_type().unwrap_or_default().starts_with("image/") => {
            Err("The map service did not return an image (the place may be outside its coverage).".into())
        }
        _ if resp.ok => Ok(resp.bytes),
        _ => Err(format!("The service answered {} {}.", resp.status, resp.status_text)),
    }
}
