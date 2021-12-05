use interception::*;

pub struct InterceptionState {
    interception: Interception,
    device: Device,
}

impl InterceptionState {
    pub fn new() -> Self {
        InterceptionState {
            interception: Interception::new().expect("Error initializing interception"),
            device: -1,
        }
    }
    pub fn capture_mouse(&mut self) {
        println!("Looking for mouse...");
        self.interception
            .set_filter(is_mouse, Filter::MouseFilter(MouseState::MOVE));
        self.device = self.interception.wait();
        self.interception
            .set_filter(is_mouse, Filter::MouseFilter(MouseState::empty()));
        println!("Found mouse");
    }
}

pub fn move_mouse_relative(ic_state: &InterceptionState, x: i32, y: i32) {
    let stroke = Stroke::Mouse {
        state: MouseState::MOVE,
        flags: MouseFlags::MOVE_RELATIVE,
        rolling: 0,
        x: x,
        y: y,
        information: 0,
    };
    ic_state.interception.send(ic_state.device, &[stroke]);
}
