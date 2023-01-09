use gstreamer::{Bin, Element, ElementFactory, GhostPad};
use gstreamer_app::{AppSink, AppSinkCallbacks};
use gstreamer_editing_services::{prelude::*, Pipeline, Timeline, UriClip};

fn main() {
    run(play)
}

fn play() {
    // Initialize GStreamer
    gstreamer::init().unwrap();
    gstreamer_editing_services::init().unwrap();

    let timeline = Timeline::new_audio_video();
    let layer = timeline.append_layer();
    let pipeline = Pipeline::new();
    pipeline.set_timeline(&timeline).unwrap();
    let clip = UriClip::new("file:///Users/itome/Downloads/bun33s.mp4").unwrap();
    layer.add_clip(&clip).unwrap();

    let bin = Bin::builder().build();
    let sink = AppSink::builder().build();
    let enc = ElementFactory::make("vp8enc").build().unwrap();
    enc.set_property("deadline", 1i64);
    enc.set_property("target-bitrate", 10240000);
    let rtpvp8pay = ElementFactory::make("rtpvp8pay").build().unwrap();
    bin.add_many(&[&enc, &rtpvp8pay, sink.upcast_ref()])
        .unwrap();
    Element::link_many(&[&enc, &rtpvp8pay, sink.upcast_ref()]).unwrap();

    let pad = enc
        .static_pad("sink")
        .expect("Failed to get a static pad from equalizer.");
    let ghost_pad = GhostPad::with_target(Some("sink"), &pad).unwrap();
    ghost_pad.set_active(true).unwrap();
    bin.add_pad(&ghost_pad).unwrap();

    sink.set_callbacks(
        AppSinkCallbacks::builder()
            .new_sample(move |appsink| {
                if let Ok(sample) = appsink.pull_sample() {
                    let buffer = sample.buffer().unwrap();
                    println!("{:?}", buffer);
                    let map = buffer.map_readable().unwrap();
                    println!("Got buffer of size {}", map.len());
                }

                Ok(gstreamer::FlowSuccess::Ok)
            })
            .build(),
    );

    pipeline.preview_set_video_sink(Some(&bin));

    // Start playing
    pipeline
        .set_state(gstreamer::State::Playing)
        .expect("Unable to set the pipeline to the `Playing` state");

    // Wait until error or EOS
    let bus = pipeline.bus().unwrap();
    for msg in bus.iter_timed(gstreamer::ClockTime::NONE) {
        use gstreamer::MessageView;

        match msg.view() {
            MessageView::Eos(..) => break,
            MessageView::Error(err) => {
                println!(
                    "Error from {:?}: {} ({:?})",
                    err.src().map(|s| s.path_string()),
                    err.error(),
                    err.debug()
                );
                break;
            }
            _ => (),
        }
    }

    // Shutdown pipeline
    pipeline
        .set_state(gstreamer::State::Null)
        .expect("Unable to set the pipeline to the `Null` state");
}

#[cfg(not(target_os = "macos"))]
pub fn run<T, F: FnOnce() -> T + Send + 'static>(main: F) -> T
where
    T: Send + 'static,
{
    main()
}

#[cfg(target_os = "macos")]
pub fn run<T, F: FnOnce() -> T + Send + 'static>(main: F) -> T
where
    T: Send + 'static,
{
    use cocoa::appkit::NSApplication;
    use objc::{msg_send, sel, sel_impl};

    use std::thread;

    unsafe {
        let app = cocoa::appkit::NSApp();
        let t = thread::spawn(|| {
            let res = main();

            let app = cocoa::appkit::NSApp();
            let _: () = msg_send![app, terminate: cocoa::base::nil];

            res
        });

        app.run();

        t.join().unwrap()
    }
}
