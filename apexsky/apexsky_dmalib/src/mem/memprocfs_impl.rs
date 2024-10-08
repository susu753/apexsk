use anyhow::{anyhow, Context};
use apexsky_utils::get_runner_home_dir;
use memprocfs::{Vmm, VmmProcess, VmmScatterMemory, FLAG_NOCACHE};
use obfstr::obfstr as s;
use once_cell::sync::Lazy;
use std::{env, sync::Arc, time::Instant};

use crate::mem::dma_helper::fix_dtb;

use super::{MemProc, ProcessStatus};

pub struct MemProcFsOs {
    vmm: Arc<Vmm<'static>>,
}

pub struct MemProcFSProc<'a> {
    base_addr: u64,
    status: ProcessStatus,
    proc: VmmProcess<'a>,
}

impl<'a> std::fmt::Debug for MemProcFSProc<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(s!("MemProcFSProc"))
            .field(s!("base_addr"), &self.base_addr)
            .field(s!("status"), &self.status)
            .field(s!("proc"), &self.proc)
            .finish()
    }
}

impl MemProcFsOs {
    pub fn new(choose_connector: super::MemConnector) -> anyhow::Result<Self> {
        let device = if let super::MemConnector::PCILeech(device) = choose_connector {
            device.to_owned()
        } else {
            return Err(anyhow!(
                "{}{:?}",
                s!("Invalid connector: "),
                choose_connector
            ));
        };

        tracing::info!("{}{}", s!("leechcore device: "), device);

        // MemProcFS Rust requires full path to vmm.dll/so
        let vmm_path: String = (|| {
            if cfg!(unix) {
                // try ~/.local/lib/memprocfs/vmm.so
                if let Some(local_lib) = get_runner_home_dir().map(|home| {
                    home.join(s!(".local"))
                        .join(s!("lib"))
                        .join(s!("memprocfs"))
                        .join(s!("vmm.so"))
                }) {
                    if local_lib.try_exists().is_ok_and(|exists| exists) {
                        return anyhow::Ok(local_lib.to_str().unwrap().to_string());
                    }
                }
            }

            let current_dir = env::current_dir()
                .map_err(|_| anyhow!("{}", s!("App: Unable to get current directory.")))?;

            if cfg!(windows) {
                Ok(current_dir
                    .join(s!("vmm.dll"))
                    .to_str()
                    .unwrap()
                    .to_string())
            } else {
                Ok(current_dir.join(s!("vmm.so")).to_str().unwrap().to_string())
            }
        })()?;

        // Initialize Vmm on passed parameters, always expect this to be ok, so panic if it's not
        let vmm = {
            let arg_waitinitialize = s!("-waitinitialize").to_owned();
            let arg_device = s!("-device").to_owned();
            let args = vec![
                "-printf",
                //"-v",
                arg_waitinitialize.as_str(),
                arg_device.as_str(),
                device.as_str(),
                //"-vm",
            ];
            Vmm::new(&vmm_path, &args)?
        };

        // Find current Windows version (useful to enable/disable certain things!)
        tracing::info!("{}{}", s!("Kernel version: "), vmm.kernel().build());

        Ok(Self { vmm: Arc::new(vmm) })
    }
}

impl super::MemOs for MemProcFsOs {
    fn open_proc<'a>(&'a mut self, name: &str) -> anyhow::Result<super::MemProcImpl<'a>> {
        let process = self.vmm.process_from_name(name)?;

        if let Ok(procinfo) = process.info() {
            println!("{}{:?}", s!("struct   -> "), procinfo);
            println!("{}{:?}", s!("pid      -> "), procinfo.pid);
            println!("{}{:?}", s!("ppid     -> "), procinfo.pid);
            println!("{}{:x}", s!("peb      -> "), procinfo.va_peb);
            println!("{}{:x}", s!("eprocess -> "), procinfo.va_eprocess);
            println!("{}{:?}", s!("name     -> "), procinfo.name);
            println!("{}{:?}", s!("longname -> "), procinfo.name_long);
            println!("{}{:?}", s!("SID      -> "), procinfo.sid);
        }

        fix_dtb(&self.vmm, &process, name).map_err(|e| {
            tracing::warn!(%e, ?e);
            e
        })?;

        let process_base = process.get_module_base(name)?;

        tracing::info!(?name, "{}{:x}", s!("module found: 0x"), process_base);

        Ok(super::MemProcImpl::Vmm(MemProcFSProc {
            base_addr: process_base,
            status: ProcessStatus::FoundReady,
            proc: process,
        }))
    }
}

impl<'a> MemProc for MemProcFSProc<'a> {
    #[inline]
    fn get_base_addr(&self) -> u64 {
        self.base_addr
    }

    fn set_base_addr(&mut self, base_addr: u64) {
        self.base_addr = base_addr;
    }

    fn check_proc_status(&mut self) -> super::ProcessStatus {
        if self.base_addr == 0 {
            self.status = ProcessStatus::NotFound;
            return self.status;
        }

        if self.status == ProcessStatus::FoundReady {
            let mut c: i16 = 0;
            self.read_raw_into(self.base_addr, dataview::bytes_mut(&mut c))
                .ok();

            if c != 0x5A4D {
                self.status = ProcessStatus::NotFound;
                self.base_addr = 0;
            }
        }

        self.status
    }

    fn speed_test(&mut self) {
        tracing::debug!("{}", s!("029194cf-ce9a-42aa-91c7-e35108e9ddb0"));
        if self.status != ProcessStatus::FoundReady {
            static ERR_MSG: Lazy<String> = Lazy::new(|| s!("proc instance is None").to_string());
            let err = anyhow!(&*ERR_MSG);
            tracing::error!(%err);
            return;
        }

        println!("{}", s!("== speed test start =="));

        let addr = self.base_addr;
        let start = Instant::now();
        for counter in 0..5000 {
            match self.proc.mem_read_ex(addr, 0x1000, FLAG_NOCACHE) {
                Ok(_data_read) => {
                    //println!("{:?}", data_read.hex_dump());
                }
                Err(e) => {
                    let err: Result<(), _> =
                        Err(e).context(s!("speed_test: unable to read process memory").to_string());
                    err.unwrap();
                }
            }

            if counter % 1000 == 0 {
                let elapsed = start.elapsed().as_millis() as f64;
                if elapsed > 0.0 {
                    let result = (
                        format!(
                            "{}{}",
                            (f64::from(counter)) / elapsed * 1000.0,
                            s!(" reads/sec")
                        ),
                        format!("{}{}", elapsed / (f64::from(counter)), s!(" ms/read")),
                    );
                    tracing::debug!(result0 = result.0, result1 = result.1);
                    println!("{}", result.0);
                    println!("{}", result.1);
                }
            }
        }

        println!("{}", s!("== speed test end =="));
    }

    fn read_raw_into(&mut self, addr: u64, out: &mut [u8]) -> anyhow::Result<()> {
        if self.status != ProcessStatus::FoundReady {
            anyhow::bail!(s!("proc instance is None").to_string());
        }
        out.copy_from_slice(&self.proc.mem_read_ex(addr, out.len(), FLAG_NOCACHE)?);
        Ok(())
    }

    fn write_raw(&mut self, addr: u64, data: &[u8]) -> anyhow::Result<()> {
        if self.status != ProcessStatus::FoundReady {
            anyhow::bail!(s!("proc instance is None").to_string());
        }
        self.proc.mem_write(addr, &data.to_vec())?;
        Ok(())
    }
}

impl<'a> MemProcFSProc<'a> {
    pub fn mem_scatter(&mut self) -> anyhow::Result<VmmScatterMemory> {
        if self.status != ProcessStatus::FoundReady {
            anyhow::bail!(s!("proc instance is None").to_string());
        }
        let mem_scatter = self.proc.mem_scatter(FLAG_NOCACHE)?;
        Ok(mem_scatter)
    }
}
