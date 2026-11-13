//! The chat drawn in the pane, so the conversation works in any terminal,
//! including Herdr's own. It is a client of the session like Sheprd is,
//! applying the same changes through the same reducer.

mod dropped;
mod lines;
mod render;
mod view;

use std::{
    io::{Write, stdout},
    thread,
    time::Duration,
};

use crossbeam_channel::{Receiver, Sender, bounded, never, select};
use crossterm::{
    event::{
        DisableBracketedPaste, EnableBracketedPaste, Event, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
};
use sheprd_chat_protocol::{CommandId, Cursor, ServerMessage};

use crate::session::{Attach, ConnId, Input};
use view::{Out, Received, View};

/// The view's own connection id; attached clients start at 1.
const LOCAL: ConnId = ConnId(0);
const QUEUE: usize = 4096;
const FRAME: Duration = Duration::from_millis(100);

pub(crate) fn run(inputs: &Sender<Input>) -> anyhow::Result<()> {
    let mut terminal = ratatui::try_init()?;
    let mut out = stdout();
    let _ = execute!(out, EnableBracketedPaste);
    let keyboard = matches!(
        crossterm::terminal::supports_keyboard_enhancement(),
        Ok(true)
    );
    if keyboard {
        let _ = execute!(
            out,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        );
    }
    let result = event_loop(&mut terminal, inputs);
    if keyboard {
        let _ = execute!(out, PopKeyboardEnhancementFlags);
    }
    let _ = execute!(out, DisableBracketedPaste);
    let _ = write!(out, "\x1b]7501;state=clear\x1b\\");
    ratatui::restore();
    result
}

fn attach(inputs: &Sender<Input>, since: Option<Cursor>) -> Receiver<ServerMessage> {
    let (outbox, receiver) = bounded(QUEUE);
    let _ = inputs.send(Input::Detach(LOCAL));
    let _ = inputs.send(Input::Attach(Attach {
        conn: LOCAL,
        local: true,
        outbox,
        since,
    }));
    receiver
}

/// Terminal events, then the read error that ended them, if any.
fn terminal_events() -> std::io::Result<Receiver<std::io::Result<Event>>> {
    let (sender, receiver) = bounded(256);
    // Blocks in `read` for the life of the process; it is not joined.
    thread::Builder::new()
        .name("tui-events".to_owned())
        .spawn(move || {
            loop {
                let event = crossterm::event::read();
                let failed = event.is_err();
                if sender.send(event).is_err() || failed {
                    return;
                }
            }
        })?;
    Ok(receiver)
}

/// A terminal event, or why there will be none. Losing the terminal is a
/// failure, never a quit: only the user's quit may forget the chat's recovery.
fn terminal_event(
    received: Result<std::io::Result<Event>, crossbeam_channel::RecvError>,
) -> crate::Result<Event> {
    match received {
        Ok(Ok(event)) => Ok(event),
        Ok(Err(source)) => Err(crate::Error::TerminalInput(source)),
        Err(crossbeam_channel::RecvError) => Err(crate::Error::TerminalInput(
            std::io::ErrorKind::UnexpectedEof.into(),
        )),
    }
}

fn event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    inputs: &Sender<Input>,
) -> anyhow::Result<()> {
    let events = terminal_events()?;
    let mut view = View::default();
    let mut session = attach(inputs, None);
    let mut tick = 0_usize;
    let mut reported = None;
    loop {
        let status = view::program_status(&view.transcript);
        if reported != Some(status) {
            reported = Some(status);
            let mut out = stdout();
            let _ = write!(out, "\x1b]7501;state={status}:app=herdr-acp\x1b\\");
            let _ = out.flush();
        }
        terminal.draw(|frame| render::draw(frame, &mut view, tick))?;
        let animate = status == "working";
        let timer = if animate {
            crossbeam_channel::after(FRAME)
        } else {
            never()
        };
        select! {
            recv(session) -> message => {
                let Ok(message) = message else {
                    // The session dropped this view for falling behind.
                    session = attach(inputs, view.cursor());
                    continue;
                };
                let mut next = Some(message);
                while let Some(message) = next {
                    match view.receive(message) {
                        Received::Applied => {}
                        Received::Resync => {
                            session = attach(inputs, None);
                            break;
                        }
                        Received::Closed => return Ok(()),
                    }
                    next = session.try_recv().ok();
                }
            }
            recv(events) -> event => match terminal_event(event)? {
                Event::Key(key) => match view.key(key) {
                    Some(Out::Quit) => return Ok(()),
                    Some(Out::Command(command)) => {
                        let id = CommandId::random();
                        let _ = inputs.send(Input::Command { command, conn: LOCAL, id });
                    }
                    None => {}
                },
                Event::Paste(text) => view.paste(&text),
                _ => {}
            },
            recv(timer) -> _ => tick = tick.wrapping_add(1),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
