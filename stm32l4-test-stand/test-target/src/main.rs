//! Firmware for the STM32L4 Test Stand


#![no_main]
#![no_std]


extern crate panic_rtt_target;


use heapless::{
    Vec,
    consts::U256,
    spsc,
};
use rtt_target::{
    rprint,
    rprintln,
};
use stm32l4xx_hal::{
    prelude::*,
    pac::{
        self,
        USART1,
        USART2,
    },
    serial::{
        self,
        Serial,
    },
};

use lpc845_messages::{
    HostToTarget,
    UsartMode,
};


#[rtic::app(device = stm32l4xx_hal::pac)]
const APP: () = {
    struct Resources {
        rx_main: serial::Rx<USART1>,
        tx_main: serial::Tx<USART1>,
        rx_host: serial::Rx<USART2>,
        tx_host: serial::Tx<USART2>,

        rx_prod_main: spsc::Producer<'static, u8, U256>,
        rx_cons_main: spsc::Consumer<'static, u8, U256>,
        rx_prod_host: spsc::Producer<'static, u8, U256>,
        rx_cons_host: spsc::Consumer<'static, u8, U256>,
    }

    #[init]
    fn init(_cx: init::Context) -> init::LateResources {
        static mut RX_QUEUE_HOST: spsc::Queue<u8, U256> =
            spsc::Queue(heapless::i::Queue::new());
        static mut RX_QUEUE_MAIN: spsc::Queue<u8, U256> =
            spsc::Queue(heapless::i::Queue::new());

        rtt_target::rtt_init_print!();
        rprint!("Starting target...");

        let p = pac::Peripherals::take().unwrap();

        let mut rcc = p.RCC.constrain();
        let mut flash = p.FLASH.constrain();
        let mut pwr = p.PWR.constrain(&mut rcc.apb1r1);

        let clocks = rcc.cfgr.freeze(&mut flash.acr, &mut pwr);

        let mut gpioa = p.GPIOA.split(&mut rcc.ahb2);
        let mut gpiob = p.GPIOB.split(&mut rcc.ahb2);

        let tx_pin_main = gpiob.pb6.into_af7(&mut gpiob.moder, &mut gpiob.afrl);
        let rx_pin_main = gpiob.pb7.into_af7(&mut gpiob.moder, &mut gpiob.afrl);
        let tx_pin_host = gpioa.pa2.into_af7(&mut gpioa.moder, &mut gpioa.afrl);
        let rx_pin_host = gpioa.pa3.into_af7(&mut gpioa.moder, &mut gpioa.afrl);

        let mut usart_main = Serial::usart1(
            p.USART1,
            (tx_pin_main, rx_pin_main),
            serial::Config::default().baudrate(115_200.bps()),
            clocks,
            &mut rcc.apb2,
        );
        let mut usart_host = Serial::usart2(
            p.USART2,
            (tx_pin_host, rx_pin_host),
            serial::Config::default().baudrate(115_200.bps()),
            clocks,
            &mut rcc.apb1r1,
        );

        usart_main.listen(serial::Event::Rxne);
        usart_host.listen(serial::Event::Rxne);

        let (tx_main, rx_main) = usart_main.split();
        let (tx_host, rx_host) = usart_host.split();
        let (rx_prod_main, rx_cons_main) = RX_QUEUE_MAIN.split();
        let (rx_prod_host, rx_cons_host) = RX_QUEUE_HOST.split();

        rprintln!("done.");

        init::LateResources {
            rx_main,
            tx_main,
            rx_host,
            tx_host,

            rx_prod_main,
            rx_cons_main,
            rx_prod_host,
            rx_cons_host,
        }
    }

    #[idle(resources = [rx_cons_main, rx_cons_host, tx_main, tx_host])]
    fn idle(cx: idle::Context) -> ! {
        let rx_main = cx.resources.rx_cons_main;
        let rx_host = cx.resources.rx_cons_host;
        let tx_main = cx.resources.tx_main;

        let mut buf_host_rx: Vec<_, U256> = Vec::new();

        loop {
            while let Some(b) = rx_main.dequeue() {
                rprintln!("Received: {}", b);
            }

            if let Some(b) = rx_host.dequeue() {
                // Requests are COBS-encoded, so we know that `0` means we
                // received a full frame.
                if b != 0 {
                    buf_host_rx.push(b).expect("Receive buffer full");
                    continue;
                }

                let message = postcard::from_bytes_cobs(&mut buf_host_rx)
                    .expect("Error decoding message");
                match message {
                    HostToTarget::SendUsart {
                        mode: UsartMode::Regular,
                        data,
                    } => {
                        tx_main.bwrite_all(data)
                            .expect("Error writing to USART");
                        rprintln!("Sent data from host: {:?}", data);
                    }
                    message => {
                        panic!("Unsupported message: {:?}", message)
                    }
                }

                buf_host_rx.clear();
            }
        }
    }

    #[task(binds = USART1, resources = [rx_main, rx_prod_main])]
    fn usart1(cx: usart1::Context) {
        let rx = cx.resources.rx_main;
        let queue = cx.resources.rx_prod_main;

        let b = match rx.read() {
            Ok(b) => b,
            Err(err) => {
                rprintln!("Error reading from USART1: {:?}", err);
                return;
            }
        };
        match queue.enqueue(b) {
            Ok(()) => (),
            Err(err) => {
                rprintln!("Error adding received byte to queue: {:?}", err);
                return;
            }
        }
    }

    #[task(binds = USART2, resources = [rx_host, rx_prod_host])]
    fn usart2(cx: usart2::Context) {
        let rx = cx.resources.rx_host;
        let queue = cx.resources.rx_prod_host;

        let b = match rx.read() {
            Ok(b) => b,
            Err(err) => {
                rprintln!("Error reading from USART2: {:?}", err);
                return;
            }
        };
        match queue.enqueue(b) {
            Ok(()) => (),
            Err(err) => {
                rprintln!("Error adding received byte to queue: {:?}", err);
                return;
            }
        }
    }
};