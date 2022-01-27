use std::ptr;
use windows::{
    core::{Error as WinError, Handle, Interface},
    Win32::{
        Foundation::{E_ACCESSDENIED, E_HANDLE},
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_UNKNOWN,
            Direct3D11::{
                D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
                D3D11_CPU_ACCESS_READ, D3D11_SDK_VERSION, D3D11_USAGE_STAGING,
            },
            Dxgi::{
                CreateDXGIFactory1, IDXGIFactory1, IDXGIOutput, IDXGIOutput1,
                IDXGIOutputDuplication, IDXGISurface, DXGI_ERROR_ACCESS_LOST,
                DXGI_ERROR_NOT_CURRENTLY_AVAILABLE, DXGI_ERROR_WAIT_TIMEOUT, DXGI_MAP_READ,
            },
        },
        System::StationsAndDesktops::{CloseDesktop, OpenInputDesktop, SetThreadDesktop},
        System::SystemServices::GENERIC_ALL,
    },
};

use crate::image::{Bgra8, Image};

#[derive(Debug)]
pub enum CaptureError {
    AccessLost,
    WinErr(WinError),
}

impl From<WinError> for CaptureError {
    fn from(e: WinError) -> Self {
        match e.code() {
            DXGI_ERROR_ACCESS_LOST => CaptureError::AccessLost,
            _ => CaptureError::WinErr(e),
        }
    }
}

pub struct DXGICapturer {
    d3d_device: ID3D11Device,
    device_context: ID3D11DeviceContext,
    primary_output: IDXGIOutput,
    output_dup: Option<IDXGIOutputDuplication>, // Should never be None,
    surface: Option<IDXGISurface>,
}

impl DXGICapturer {
    pub fn new() -> Result<Self, CaptureError> {
        unsafe {
            let input_desktop_h = OpenInputDesktop(0, false, GENERIC_ALL);
            if input_desktop_h.is_invalid() {
                return Err(WinError::new(E_HANDLE, "OpenInputDesktop bad handle".into()).into());
            }
            SetThreadDesktop(input_desktop_h); // don't care if this fails
            CloseDesktop(input_desktop_h);

            let primary_adapter = CreateDXGIFactory1::<IDXGIFactory1>()?.EnumAdapters(0)?;
            let primary_output = primary_adapter.EnumOutputs(0)?;

            let mut d3d_device = None;
            let mut device_context = None;
            D3D11CreateDevice(
                primary_adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                None,
                0.into(),
                ptr::null(),
                0,
                D3D11_SDK_VERSION,
                &mut d3d_device,
                ptr::null_mut(),
                &mut device_context,
            )?;

            let output_dup =
                Self::duplicate_output(d3d_device.as_ref().unwrap(), primary_output.clone())?;

            Ok(Self {
                d3d_device: d3d_device.unwrap(),
                device_context: device_context.unwrap(),
                primary_output,
                output_dup: Some(output_dup),
                surface: None,
            })
        }
    }

    pub fn reload(&mut self) -> Result<(), CaptureError> {
        // releasing old duplication before creating new one to avoid hitting the hard limit
        unsafe { self.release_resources()? };
        let invalid_dup = std::mem::take(&mut self.output_dup);
        drop(invalid_dup);

        loop {
            match unsafe { Self::duplicate_output(&self.d3d_device, self.primary_output.clone()) } {
                Ok(out) => {
                    self.output_dup = Some(out);
                    break;
                }

                // Access denied when system is switching between fullscreen modes, we keep retrying until it's finished switching.
                // Shouldn't loop infinitely, since E_ACCESSDENIED would be caught in the constructor.
                Err(e) if e.code() == E_ACCESSDENIED => continue,
                Err(e) => return Err(e.into()),
            }
        }

        Ok(())
    }

    // Lifetimes should guarantee never having an image that references an unmapped surface
    pub fn capture_frame(
        &'_ mut self,
        timeout_ms: u32,
    ) -> Result<Option<Image<&'_ [u8], Bgra8>>, CaptureError> {
        unsafe {
            self.release_resources()?;

            let mut desktop_resource = None;
            let mut frame_info = Default::default();
            if let Err(e) = self.output_dup.as_ref().unwrap().AcquireNextFrame(
                timeout_ms,
                &mut frame_info,
                &mut desktop_resource,
            ) {
                return match e.code() {
                    DXGI_ERROR_WAIT_TIMEOUT => Ok(None),
                    _ => Err(e.into()),
                };
            }

            let gpu_tex = desktop_resource.unwrap().cast::<ID3D11Texture2D>().unwrap();

            let mut desc = Default::default();
            gpu_tex.GetDesc(&mut desc);
            desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
            desc.Usage = D3D11_USAGE_STAGING;
            desc.BindFlags = 0.into();
            desc.MiscFlags = 0.into();

            let cpu_tex = self.d3d_device.CreateTexture2D(&desc, ptr::null())?;
            self.device_context.CopyResource(&cpu_tex, &gpu_tex);

            let mut rect = Default::default();
            let surface = cpu_tex.cast::<IDXGISurface>().unwrap();
            surface.Map(&mut rect, DXGI_MAP_READ)?;
            self.surface = Some(surface);

            // always in BGRA8 format
            let (w, h) = (desc.Width as usize, desc.Height as usize);
            let pixels_slice = std::slice::from_raw_parts(rect.pBits, w * h * 4);

            Ok(Some(Image::new(pixels_slice, w, h)))
        }
    }

    pub fn dims(&self) -> (u32, u32) {
        let mut desc = Default::default();
        unsafe { self.output_dup.as_ref().unwrap().GetDesc(&mut desc) };
        (desc.ModeDesc.Width, desc.ModeDesc.Height)
    }

    unsafe fn release_resources(&mut self) -> Result<(), WinError> {
        if let Some(ref mut surf) = self.surface {
            surf.Unmap()?;
            self.surface = None;
            self.output_dup.as_ref().unwrap().ReleaseFrame()?
        }
        Ok(())
    }

    unsafe fn duplicate_output(
        d3d_device: &ID3D11Device,
        output: IDXGIOutput,
    ) -> Result<IDXGIOutputDuplication, WinError> {
        output
            .cast::<IDXGIOutput1>()
            .unwrap()
            .DuplicateOutput(d3d_device)
            .map_err(|e| match e.code() {
                DXGI_ERROR_NOT_CURRENTLY_AVAILABLE => {
                    WinError::new(e.code(), "Max # of apps using duplication api".into())
                }
                _ => e,
            })
    }
}
