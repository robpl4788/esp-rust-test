#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use defmt::info;
use defmt::println;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_hal::clock::CpuClock;
use esp_hal::efuse::Efuse;
use esp_hal::gpio;
use esp_hal::gpio::Input;
use esp_hal::rmt::Tx;
use esp_hal::timer::timg::TimerGroup;
use esp_hal::gpio::Output;
use esp_hal::usb_serial_jtag::{UsbSerialJtag, UsbSerialJtagRx, UsbSerialJtagTx};
use esp_radio::esp_now::EspNow;
use esp_radio::esp_now::EspNowReceiver;
use esp_radio::esp_now::EspNowSender;
use {esp_backtrace as _, esp_println as _};
use esp_alloc as _;
use esp_backtrace as _;
use esp_radio::Controller;
use esp_radio::esp_now::{BROADCAST_ADDRESS, PeerInfo};
use esp_hal::Async;

pub const RX_ADDRESS: [u8; 6] = [184u8, 248u8,  98u8,  46u8,  97u8,  52u8];
pub const TX_ADDRESS: [u8; 6] = [ 80u8, 120u8, 125u8,  76u8, 157u8, 128u8];


// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]

#[embassy_executor::task]
async fn esp_now_send(mut esp_now: EspNowSender<'static>, button: Input<'static>) -> ! {
    let mut x: u8 = 0;
    loop {
        if button.is_high(){
            if Efuse::read_base_mac_address() == RX_ADDRESS {
                esp_now.send_async(&TX_ADDRESS, &[x, 1]).await;

            } else {
                esp_now.send_async(&RX_ADDRESS, &[x, 1]).await;

            }
            println!("1");
        } else {
            println!("0");
            if Efuse::read_base_mac_address() == RX_ADDRESS {
                esp_now.send_async(&TX_ADDRESS, &[x, 0]).await;

            } else {
                esp_now.send_async(&RX_ADDRESS, &[x, 0]).await;

            }
        }
        x = x.wrapping_add(1);
        Timer::after(Duration::from_millis(100)).await;

    }
}

#[embassy_executor::task]
async fn esp_now_recieve(mut esp_now: EspNowReceiver<'static>, mut led: Output<'static>) -> ! {

    loop {
        let r = esp_now.receive_async().await;
        if r.data()[1] == 0 {
            led.set_high();
        } else {
            led.set_low();
        }
        println!("Received {:?}", r.data());
            // println!("Recieved Address {:?}", r.info.src_address == OTHER_ADDRESS);
    }
}


// #[embassy_executor::task]
// async fn uart_reader(mut uart: Uart<'static, UART0>) {
//     let mut buf = [0u8; 64];

//     loop {
//         match uart.read(&mut buf).await {
//             Ok(n) => {
//                 let received = core::str::from_utf8(&buf[..n]).unwrap_or("<invalid>");
//                 println!("Got from terminal: {}", received);
//             }
//             Err(e) => {
//                 println!("UART error: {:?}", e);
//             }
//         }
//     }
// }

#[embassy_executor::task]
async fn toggle_led(mut led: Output<'static>) -> ! {

    loop {
        led.toggle();
        Timer::after(Duration::from_millis(500)).await;
    }
}

#[embassy_executor::task]
async fn alive_signal() {
    loop {
        println!("I'm alive, yay");
        Timer::after(Duration::from_millis(700)).await;
    }
}

macro_rules! mk_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}

#[embassy_executor::task]
async fn reader(
    mut rx: UsbSerialJtagRx<'static, Async>,
) {
    let mut rbuf = [0u8; 5];
    loop {
        let r = embedded_io_async::Read::read(&mut rx, &mut rbuf).await;
        match r {
            Ok(len) => {
                let mut i = 0;
                while i < len {
                    println!("Got string: {:?}", char::from(rbuf[i]));

                    i += 1;
                }
            }
            #[allow(unreachable_patterns)]
            Err(e) => esp_println::println!("RX Error: {:?}", e),
        }
    }
}


#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    // generator version: 1.2.0

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 66320);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_interrupt =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_interrupt.software_interrupt0);

    let (rx, tx) = UsbSerialJtag::new(peripherals.USB_DEVICE)
        .into_async()
        .split();

    let esp_radio_ctrl = &*mk_static!(Controller<'static>, esp_radio::init().unwrap());

    let wifi = peripherals.WIFI;
    let (mut controller, interfaces) =
        esp_radio::wifi::new(&esp_radio_ctrl, wifi, Default::default()).unwrap();
    controller.set_mode(esp_radio::wifi::WifiMode::Sta).unwrap();
    controller.start().unwrap();

    interfaces.esp_now.set_rate(esp_radio::esp_now::WifiPhyRate::Rate1mL).unwrap();
    let (esp_now_manager, esp_now_sender, esp_now_reciever) = interfaces.esp_now.split()    ;
    esp_now_manager.set_channel(1).unwrap();
    if Efuse::read_base_mac_address() == RX_ADDRESS {
        esp_now_manager
            .add_peer(PeerInfo {
                interface: esp_radio::esp_now::EspNowWifiInterface::Sta,
                peer_address: TX_ADDRESS,
                lmk: None,
                channel: None,
                encrypt: false,
            })
            .unwrap();
    } else {
        esp_now_manager
            .add_peer(PeerInfo {
                interface: esp_radio::esp_now::EspNowWifiInterface::Sta,
                peer_address: RX_ADDRESS,
                lmk: None,
                channel: None,
                encrypt: false,
            })
            .unwrap();
    }
    println!("esp-now version {}", esp_now_manager.version().unwrap());

    let led = Output::new(peripherals.GPIO4, esp_hal::gpio::Level::High, gpio::OutputConfig::default());
    let button = Input::new(peripherals.GPIO9, gpio::InputConfig::default().with_pull(gpio::Pull::Up));

    // spawner.spawn(toggle_led(led)).unwrap();
    // spawner.spawn(alive_signal()).unwrap();
    spawner.spawn(esp_now_recieve(esp_now_reciever, led)).unwrap();
    spawner.spawn(esp_now_send(esp_now_sender, button)).unwrap();
    // spawner.spawn(reader(rx)).unwrap();
    loop {
        info!("{:?}", Efuse::read_base_mac_address());
        Timer::after(Duration::from_secs(1)).await;
    }

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/esp-hal-v1.0.0/examples
}
