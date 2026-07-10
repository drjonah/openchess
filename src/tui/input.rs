//! Keybindings — command keys avoid chess files `a`–`h`.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputAction {
    Quit,
    Help,
    OpenSettings,
    NewGame,
    Undo,
    Flip,
    EngineGo,
    Stop,
    ModePlayerVsPlayer,
    ModePlayerVsBotWhite,
    ModePlayerVsBotBlack,
    ModeBotVsBot,
    ModeAnalyze,
    StartImport,
    CancelImport,
    StepBack,
    StepForward,
    GotoStart,
    GotoEnd,
    ToggleEvalBar,
    Submit(String),
    #[cfg(feature = "chesscom")]
    ListUp,
    #[cfg(feature = "chesscom")]
    ListDown,
    #[cfg(feature = "chesscom")]
    SelectGame,
    /// Leave game list and return to username import prompt.
    #[cfg(feature = "chesscom")]
    CancelGameList,
    /// Reveal the next page of cached chess.com games.
    #[cfg(feature = "chesscom")]
    LoadMoreGames,
    /// Re-fetch games from chess.com and overwrite the disk cache.
    #[cfg(feature = "chesscom")]
    RefreshGames,
    Redraw,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PromptKind {
    #[default]
    Move,
    Import,
    #[cfg(feature = "chesscom")]
    GameList,
}

pub const HELP_TEXT: &str = "\
OpenChess TUI

  GAME
    n new · u undo · t flip · i import · v eval bar · , settings · ? help · q quit

  SETTINGS (press ,)
    ↑↓ / j k     select field
    ←→ / - +     adjust value
    Enter / Space toggle or cycle
    Esc / ,      close (saves on each change)
    Advanced engine options: edit the JSON path shown in settings

  IMPORT (press i, then Enter)
    FEN, PGN text, .fen/.pgn file path
    game URL          https://www.chess.com/game/live/…
    username          hikaru  (browse games; needs --features chesscom)
    user:NAME         user:gmdrj
    member URL        https://www.chess.com/member/gmdrj

  CHESS.COM GAME LIST (with --features chesscom)
    ↑↓ / j k     move selection
    m            show next 10 from session cache
    r            refresh from chess.com (overwrite cache)
    Enter        load selected game (resets board)
    Esc          back to username entry

  BROWSE (after importing a game)
    Left / [     previous ply
    Right / ]    next ply
    Home         jump to start
    End / $      jump to end
    u            step back (while browsing)
    eval bar     shown automatically while browsing; v toggles otherwise

  MODES
    p   Player vs Player — you move both colors
    w   Player vs Bot    — you White, bot replies
    k   Player vs Bot    — you Black, bot replies
    x   Bot vs Bot       — engine plays both sides
    y   Analyze          — show best move, do NOT play it

  ENGINE
    g   go / think now
    s   stop thinking

  HOW TO WRITE MOVES (one move + Enter)
    Pawn     destination only:     e4  d5
             capture:              exd5
             promote:              e8=Q  (or e7e8q)
    Knight   N + square:           Nf3  Na6
             disambiguate:         Nbd2  N1c3
             capture:              Nxe5
    Bishop   B + square:           Bc4  Bb5
             capture:              Bxe6
    Rook     R + square:           Re1  Rh3
             disambiguate:         Rad1  R1e2
             capture:              Rxe4
    Queen    Q + square:           Qh4  Qd2
             capture:              Qxf7
    King     K + square:           Ke2  Kf1
             capture:              Kxe2
    Castle   kingside / queenside: O-O  O-O-O
    Or use UCI always:             e2e4  g1f3  e1g1

    vs bot: enter YOUR move only — the bot replies
";

#[derive(Debug, Default)]
pub struct MoveInput {
    buffer: String,
    prompt: PromptKind,
}

impl MoveInput {
    pub fn text(&self) -> &str {
        &self.buffer
    }

    pub fn prompt(&self) -> PromptKind {
        self.prompt
    }

    pub fn start_import(&mut self) {
        self.buffer.clear();
        self.prompt = PromptKind::Import;
    }

    pub fn cancel_import(&mut self) {
        self.buffer.clear();
        self.prompt = PromptKind::Move;
    }

    /// After a failed username fetch, stay in Import so the user can retry.
    pub fn reopen_import(&mut self) {
        self.buffer.clear();
        self.prompt = PromptKind::Import;
    }

    #[cfg(feature = "chesscom")]
    pub fn start_game_list(&mut self) {
        self.buffer.clear();
        self.prompt = PromptKind::GameList;
    }

    /// Esc from game list → type a different username.
    #[cfg(feature = "chesscom")]
    pub fn back_to_import(&mut self) {
        self.buffer.clear();
        self.prompt = PromptKind::Import;
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<InputAction> {
        match self.prompt {
            PromptKind::Import => return self.handle_import_key(key),
            #[cfg(feature = "chesscom")]
            PromptKind::GameList => return self.handle_game_list_key(key),
            PromptKind::Move => {}
        }
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(InputAction::Quit)
            }
            KeyCode::Esc if self.buffer.is_empty() => Some(InputAction::Quit),
            KeyCode::Char('?') if self.buffer.is_empty() => Some(InputAction::Help),
            KeyCode::Char(',') if self.buffer.is_empty() => Some(InputAction::OpenSettings),
            KeyCode::Char('q') if self.buffer.is_empty() => Some(InputAction::Quit),
            KeyCode::Char('n') if self.buffer.is_empty() => Some(InputAction::NewGame),
            KeyCode::Char('u') if self.buffer.is_empty() => Some(InputAction::Undo),
            KeyCode::Char('t') if self.buffer.is_empty() => Some(InputAction::Flip),
            KeyCode::Char('g') if self.buffer.is_empty() => Some(InputAction::EngineGo),
            KeyCode::Char('s') if self.buffer.is_empty() => Some(InputAction::Stop),
            KeyCode::Char('p') if self.buffer.is_empty() => Some(InputAction::ModePlayerVsPlayer),
            KeyCode::Char('w') if self.buffer.is_empty() => Some(InputAction::ModePlayerVsBotWhite),
            KeyCode::Char('k') if self.buffer.is_empty() => Some(InputAction::ModePlayerVsBotBlack),
            KeyCode::Char('x') if self.buffer.is_empty() => Some(InputAction::ModeBotVsBot),
            KeyCode::Char('y') if self.buffer.is_empty() => Some(InputAction::ModeAnalyze),
            KeyCode::Char('i') if self.buffer.is_empty() => Some(InputAction::StartImport),
            KeyCode::Char('v') if self.buffer.is_empty() => Some(InputAction::ToggleEvalBar),
            KeyCode::Left if self.buffer.is_empty() => Some(InputAction::StepBack),
            KeyCode::Right if self.buffer.is_empty() => Some(InputAction::StepForward),
            KeyCode::Char('[') if self.buffer.is_empty() => Some(InputAction::StepBack),
            KeyCode::Char(']') if self.buffer.is_empty() => Some(InputAction::StepForward),
            KeyCode::Home if self.buffer.is_empty() => Some(InputAction::GotoStart),
            KeyCode::End if self.buffer.is_empty() => Some(InputAction::GotoEnd),
            KeyCode::Char('$') if self.buffer.is_empty() => Some(InputAction::GotoEnd),
            KeyCode::Enter => {
                let text = self.buffer.trim().to_string();
                self.buffer.clear();
                if text.is_empty() {
                    Some(InputAction::Redraw)
                } else {
                    Some(InputAction::Submit(text))
                }
            }
            KeyCode::Backspace => {
                self.buffer.pop();
                Some(InputAction::Redraw)
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                // Keep case so SAN piece letters work (Nf3). Files are typed lowercase.
                if c.is_ascii_alphanumeric() || c == '-' || c == '=' {
                    self.buffer.push(c);
                    Some(InputAction::Redraw)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn handle_import_key(&mut self, key: KeyEvent) -> Option<InputAction> {
        match key.code {
            KeyCode::Esc => Some(InputAction::CancelImport),
            KeyCode::Enter => {
                let text = self.buffer.trim().to_string();
                self.buffer.clear();
                // Leave Import temporarily; caller reopens on username error
                // or switches to GameList on success.
                self.prompt = PromptKind::Move;
                if text.is_empty() {
                    Some(InputAction::CancelImport)
                } else {
                    Some(InputAction::Submit(text))
                }
            }
            KeyCode::Backspace => {
                self.buffer.pop();
                Some(InputAction::Redraw)
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if !c.is_control() {
                    self.buffer.push(c);
                    Some(InputAction::Redraw)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    #[cfg(feature = "chesscom")]
    fn handle_game_list_key(&mut self, key: KeyEvent) -> Option<InputAction> {
        match key.code {
            KeyCode::Esc => Some(InputAction::CancelGameList),
            KeyCode::Enter => Some(InputAction::SelectGame),
            KeyCode::Up | KeyCode::Char('k') => Some(InputAction::ListUp),
            KeyCode::Down | KeyCode::Char('j') => Some(InputAction::ListDown),
            KeyCode::Char('m') => Some(InputAction::LoadMoreGames),
            KeyCode::Char('r') => Some(InputAction::RefreshGames),
            _ => None,
        }
    }
}
