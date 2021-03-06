// {{{ Module docs
//! `slog-rs`'a `Drain` for terminal output
//!
//! This crate implements output formatting targeting logging to
//! terminal/console/shell or similar text-based IO.
//!
//! Using `Decorator` open trait, user can implement outputting
//! using different colors, terminal types and so on.
//!
//! ```
//! #[macro_use]
//! extern crate slog;
//! extern crate slog_term;
//!
//! use slog::*;
//!
//! fn main() {
//!     let plain = slog_term::PlainSyncDecorator::new(std::io::stdout());
//!     let root = Logger::root(
//!         slog_term::FullFormat::new(plain)
//!         .build().fuse(), o!()
//!     );
//! }
//! ```
// }}}

// {{{ Imports & meta
#![warn(missing_docs)]

extern crate slog;
extern crate isatty;
extern crate chrono;
extern crate thread_local;
extern crate term;

use slog::*;
use slog::Drain;

use std::{io, fmt, sync, mem};
use std::cell::RefCell;
use std::io::Write as IoWrite;
use std::panic::{UnwindSafe, RefUnwindSafe};
use std::result;
// }}}

// {{{ Decorator
/// Output decorator
///
/// Trait implementing strategy of output formating in terms of IO,
/// colors, etc.
pub trait Decorator {
    /// Get a `RecordDecorator` for a given `record`
    ///
    /// This allows `Decorator` to have on-stack data per processed `Record`s
    ///
    fn with_record<F>(&self,
                      _record: &Record,
                      _logger_values: &OwnedKVList,
                      f: F)
                      -> io::Result<()>
        where F: FnOnce(&mut RecordDecorator) -> io::Result<()>;
}

impl<T: ?Sized> Decorator for Box<T>
    where T: Decorator
{
    fn with_record<F>(&self,
                      record: &Record,
                      logger_kv: &OwnedKVList,
                      f: F)
                      -> io::Result<()>
        where F: FnOnce(&mut RecordDecorator) -> io::Result<()>
    {
        (**self).with_record(record, logger_kv, f)
    }
}

/// Per-record decorator
pub trait RecordDecorator: io::Write {
    /// Reset formatting to defaults
    fn reset(&mut self) -> io::Result<()>;

    /// Format normal text
    fn start_whitespace(&mut self) -> io::Result<()> {
        self.reset()
    }

    /// Format `Record` message
    fn start_msg(&mut self) -> io::Result<()> {
        self.reset()
    }

    /// Format timestamp
    fn start_timestamp(&mut self) -> io::Result<()> {
        self.reset()
    }

    /// Format `Record` level
    fn start_level(&mut self) -> io::Result<()> {
        self.reset()
    }

    /// Format a comma between key-value pairs
    fn start_comma(&mut self) -> io::Result<()> {
        self.reset()
    }

    /// Format key
    fn start_key(&mut self) -> io::Result<()> {
        self.reset()
    }

    /// Format a value
    fn start_value(&mut self) -> io::Result<()> {
        self.reset()
    }

    /// Format value
    fn start_separator(&mut self) -> io::Result<()> {
        self.reset()
    }
}

impl RecordDecorator for Box<RecordDecorator> {
    fn reset(&mut self) -> io::Result<()> {
        (**self).reset()
    }
    fn start_whitespace(&mut self) -> io::Result<()> {
        (**self).start_whitespace()
    }

    /// Format `Record` message
    fn start_msg(&mut self) -> io::Result<()> {
        (**self).start_msg()
    }

    /// Format timestamp
    fn start_timestamp(&mut self) -> io::Result<()> {
        (**self).start_timestamp()
    }

    /// Format `Record` level
    fn start_level(&mut self) -> io::Result<()> {
        (**self).start_level()
    }

    /// Format `Record` message
    fn start_comma(&mut self) -> io::Result<()> {
        (**self).start_comma()
    }

    /// Format key
    fn start_key(&mut self) -> io::Result<()> {
        (**self).start_key()
    }

    /// Format value
    fn start_value(&mut self) -> io::Result<()> {
        (**self).start_value()
    }

    /// Format value
    fn start_separator(&mut self) -> io::Result<()> {
        (**self).start_separator()
    }
}
// }}}

// {{{ Misc
/// Returns `true` if message was not empty
fn print_msg_header(fn_timestamp: &ThreadSafeTimestampFn<Output =
                    io::Result<()>>, mut rd: &mut RecordDecorator, record:
                    &Record)
-> io::Result<bool>{
    try!(rd.start_timestamp());
    try!(fn_timestamp(&mut rd));

    try!(rd.start_whitespace());
    try!(write!(rd, " "));

    try!(rd.start_level());
    try!(write!(rd, "{}", record.level().as_short_str()));

    try!(rd.start_whitespace());
    try!(write!(rd, " "));

    try!(rd.start_msg());
    let mut count_rd = CountingWriter::new(&mut rd);
    try!(write!(count_rd, "{}", record.msg()));
    Ok(count_rd.count() != 0)
}

// }}}

// {{{ Term
/// Terminal-output formatting `Drain`
pub struct FullFormat<D>
    where D: Decorator
{
    decorator: D,
    fn_timestamp: Box<ThreadSafeTimestampFn<Output = io::Result<()>>>,
}

/// Streamer builder
pub struct FullFormatBuilder<D>
    where D: Decorator
{
    decorator: D,
    fn_timestamp: Box<ThreadSafeTimestampFn<Output = io::Result<()>>>,
}

impl<D> FullFormatBuilder<D>
    where D: Decorator
{
    /// Use the UTC time zone for the timestamp
    pub fn use_utc_timestamp(mut self) -> Self {
        self.fn_timestamp = Box::new(timestamp_utc);
        self
    }

    /// Use the local time zone for the timestamp (default)
    pub fn use_local_timestamp(mut self) -> Self {
        self.fn_timestamp = Box::new(timestamp_local);
        self
    }

    /// Provide a custom function to generate the timestamp
    pub fn use_custom_timestamp<F>(mut self, f: F) -> Self
        where F: ThreadSafeTimestampFn
    {
        self.fn_timestamp = Box::new(f);
        self
    }

    /// Build `FullFormat`
    pub fn build(self) -> FullFormat<D> {
        FullFormat {
            decorator: self.decorator,
            fn_timestamp: self.fn_timestamp,
        }
    }
}


impl<D> Drain for FullFormat<D>
    where D: Decorator
{
    type Ok = ();
    type Err = io::Error;

    fn log(&self,
           record: &Record,
           values: &OwnedKVList)
           -> result::Result<Self::Ok, Self::Err> {
        self.format_full(record, values)
    }
}

impl<D> FullFormat<D>
    where D: Decorator
{
    /// New `TermBuilder`
    pub fn new(d: D) -> FullFormatBuilder<D> {
        FullFormatBuilder {
            fn_timestamp: Box::new(timestamp_local),
            decorator: d,
        }
    }

    fn format_full(&self,
                   record: &Record,
                   values: &OwnedKVList)
                   -> io::Result<()> {

        self.decorator
            .with_record(record, values, |decorator| {

                let comma_needed = try!(print_msg_header(&*self.fn_timestamp,
                                                         decorator,
                                                         record));
                {
                    let mut serializer = Serializer::new(decorator,
                                                         comma_needed);

                    try!(record.kv().serialize(record, &mut serializer));

                    try!(values.serialize(record, &mut serializer));

                }

                try!(decorator.start_whitespace());
                try!(write!(decorator, "\n"));

                try!(decorator.flush());

                Ok(())
            })
    }
}
// }}}

// {{{ CompactFormat
/// Compact terminal-output formatting `Drain`
pub struct CompactFormat<D>
    where D: Decorator
{
    decorator: D,
    history: RefCell<Vec<(Vec<u8>, Vec<u8>)>>,
    fn_timestamp: Box<ThreadSafeTimestampFn<Output = io::Result<()>>>,
}

/// Streamer builder
pub struct CompactFormatBuilder<D>
    where D: Decorator
{
    decorator: D,
    fn_timestamp: Box<ThreadSafeTimestampFn<Output = io::Result<()>>>,
}

impl<D> CompactFormatBuilder<D>
    where D: Decorator
{
    /// Use the UTC time zone for the timestamp
    pub fn use_utc_timestamp(mut self) -> Self {
        self.fn_timestamp = Box::new(timestamp_utc);
        self
    }

    /// Use the local time zone for the timestamp (default)
    pub fn use_local_timestamp(mut self) -> Self {
        self.fn_timestamp = Box::new(timestamp_local);
        self
    }

    /// Provide a custom function to generate the timestamp
    pub fn use_custom_timestamp<F>(mut self, f: F) -> Self
        where F: ThreadSafeTimestampFn
    {
        self.fn_timestamp = Box::new(f);
        self
    }

    /// Build the streamer
    pub fn build(self) -> CompactFormat<D> {
        CompactFormat {
            decorator: self.decorator,
            fn_timestamp: self.fn_timestamp,
            history: RefCell::new(vec![]),
        }
    }
}


impl<D> Drain for CompactFormat<D>
    where D: Decorator
{
    type Ok = ();
    type Err = io::Error;

    fn log(&self,
           record: &Record,
           values: &OwnedKVList)
           -> result::Result<Self::Ok, Self::Err> {
        self.format_compact(record, values)
    }
}

impl<D> CompactFormat<D>
    where D: Decorator
{
    /// New `CompactFormatBuilder`
    pub fn new(d: D) -> CompactFormatBuilder<D> {
        CompactFormatBuilder {
            fn_timestamp: Box::new(timestamp_local),
            decorator: d,
        }
    }

    fn format_compact(&self,
                      record: &Record,
                      values: &OwnedKVList)
                      -> io::Result<()> {

        self.decorator
            .with_record(record, values, |decorator| {
                let indent = {
                    let mut history_ref = self.history.borrow_mut();
                    let mut serializer =
                        CompactFormatSerializer::new(decorator,
                                                     &mut *history_ref);

                    try!(values.serialize(record, &mut serializer));

                    try!(serializer.finish())
                };


                decorator.start_whitespace()?;

                for _ in 0..indent {
                    write!(decorator, " ")?;
                }

                let comma_needed = try!(print_msg_header(&*self.fn_timestamp,
                                                         decorator,
                                                         record));
                {
                    let mut serializer = Serializer::new(decorator,
                                                         comma_needed);

                    try!(record.kv().serialize(record, &mut serializer));
                }

                try!(decorator.start_whitespace());
                try!(write!(decorator, "\n"));

                try!(decorator.flush());

                Ok(())
            })
    }
}
// }}}

// {{{ Serializer
struct Serializer<'a> {
    comma_needed: bool,
    decorator: &'a mut RecordDecorator,
}

impl<'a> Serializer<'a> {
    fn new(d: &'a mut RecordDecorator, comma_needed: bool) -> Self {
        Serializer {
            comma_needed: comma_needed,
            decorator: d,
        }
    }

    fn maybe_print_comma(&mut self) -> io::Result<()> {
        if self.comma_needed {
            self.decorator.start_comma()?;
            write!(self.decorator, ", ")?;
        }
        self.comma_needed |= true;
        Ok(())
    }
}

macro_rules! s(
    ($s:expr, $k:expr, $v:expr) => {

        try!($s.maybe_print_comma());
        try!($s.decorator.start_key());
        try!(write!($s.decorator, "{}", $k));
        try!($s.decorator.start_separator());
        try!(write!($s.decorator, ":"));
        try!($s.decorator.start_whitespace());
        try!(write!($s.decorator, " "));
        try!($s.decorator.start_value());
        try!(write!($s.decorator, "{}", $v));
    };
);


impl<'a> slog::ser::Serializer for Serializer<'a> {
    fn emit_none(&mut self, key: &str) -> slog::Result {
        s!(self, key, "None");
        Ok(())
    }
    fn emit_unit(&mut self, key: &str) -> slog::Result {
        s!(self, key, "()");
        Ok(())
    }

    fn emit_bool(&mut self, key: &str, val: bool) -> slog::Result {
        s!(self, key, val);
        Ok(())
    }

    fn emit_char(&mut self, key: &str, val: char) -> slog::Result {
        s!(self, key, val);
        Ok(())
    }

    fn emit_usize(&mut self, key: &str, val: usize) -> slog::Result {
        s!(self, key, val);
        Ok(())
    }
    fn emit_isize(&mut self, key: &str, val: isize) -> slog::Result {
        s!(self, key, val);
        Ok(())
    }

    fn emit_u8(&mut self, key: &str, val: u8) -> slog::Result {
        s!(self, key, val);
        Ok(())
    }
    fn emit_i8(&mut self, key: &str, val: i8) -> slog::Result {
        s!(self, key, val);
        Ok(())
    }
    fn emit_u16(&mut self, key: &str, val: u16) -> slog::Result {
        s!(self, key, val);
        Ok(())
    }
    fn emit_i16(&mut self, key: &str, val: i16) -> slog::Result {
        s!(self, key, val);
        Ok(())
    }
    fn emit_u32(&mut self, key: &str, val: u32) -> slog::Result {
        s!(self, key, val);
        Ok(())
    }
    fn emit_i32(&mut self, key: &str, val: i32) -> slog::Result {
        s!(self, key, val);
        Ok(())
    }
    fn emit_f32(&mut self, key: &str, val: f32) -> slog::Result {
        s!(self, key, val);
        Ok(())
    }
    fn emit_u64(&mut self, key: &str, val: u64) -> slog::Result {
        s!(self, key, val);
        Ok(())
    }
    fn emit_i64(&mut self, key: &str, val: i64) -> slog::Result {
        s!(self, key, val);
        Ok(())
    }
    fn emit_f64(&mut self, key: &str, val: f64) -> slog::Result {
        s!(self, key, val);
        Ok(())
    }
    fn emit_str(&mut self, key: &str, val: &str) -> slog::Result {
        s!(self, key, val);
        Ok(())
    }
    fn emit_arguments(&mut self,
                      key: &str,
                      val: &fmt::Arguments)
                      -> slog::Result {
        s!(self, key, val);
        Ok(())
    }
}
// }}}

// {{{ CompactFormatSerializer

struct CompactFormatSerializer<'a> {
    decorator: &'a mut RecordDecorator,
    history: &'a mut Vec<(Vec<u8>, Vec<u8>)>,
    buf: Vec<(Vec<u8>, Vec<u8>)>,
}

impl<'a> CompactFormatSerializer<'a> {
    fn new(d: &'a mut RecordDecorator,
           history: &'a mut Vec<(Vec<u8>, Vec<u8>)>)
           -> Self {
        CompactFormatSerializer {
            decorator: d,
            history: history,
            buf: vec![],
        }
    }

    fn finish(&mut self) -> io::Result<usize> {
        let mut indent = 0;

        for mut buf in self.buf.drain(..).rev() {

            let (print, trunc, push) = if let Some(prev) =
                self.history.get_mut(indent) {
                if *prev != buf {
                    *prev = mem::replace(&mut buf, (vec![], vec![]));
                    (true, true, false)
                } else {
                    (false, false, false)
                }
            } else {
                (true, false, true)
            };

            if push {
                self.history
                    .push(mem::replace(&mut buf, (vec![], vec![])));

            }

            if trunc {
                self.history.truncate(indent + 1);
            }

            if print {
                let &(ref k, ref v) =
                    self.history.get(indent).expect("assertion failed");
                try!(self.decorator.start_whitespace());
                for _ in 0..indent {
                    try!(write!(self.decorator, " "));
                }
                try!(self.decorator.start_key());
                try!(self.decorator.write_all(k));
                try!(self.decorator.start_separator());
                try!(write!(self.decorator, ":"));
                try!(self.decorator.start_whitespace());
                try!(write!(self.decorator, " "));
                try!(self.decorator.start_value());
                try!(self.decorator.write_all(v));

                try!(self.decorator.start_whitespace());
                try!(write!(self.decorator, "\n"));
            }

            indent += 1;
        }

        Ok(indent)
    }
}

macro_rules! cs(
    ($s:expr, $k:expr, $v:expr) => {

        let mut k = vec!();
        let mut v = vec!();
        try!(write!(&mut k, "{}", $k));
        try!(write!(&mut v, "{}", $v));
        $s.buf.push((k, v));
    };
);


impl<'a> slog::ser::Serializer for CompactFormatSerializer<'a> {
    fn emit_none(&mut self, key: &str) -> slog::Result {
        cs!(self, key, "None");
        Ok(())
    }
    fn emit_unit(&mut self, key: &str) -> slog::Result {
        cs!(self, key, "()");
        Ok(())
    }

    fn emit_bool(&mut self, key: &str, val: bool) -> slog::Result {
        cs!(self, key, val);
        Ok(())
    }

    fn emit_char(&mut self, key: &str, val: char) -> slog::Result {
        cs!(self, key, val);
        Ok(())
    }

    fn emit_usize(&mut self, key: &str, val: usize) -> slog::Result {
        cs!(self, key, val);
        Ok(())
    }
    fn emit_isize(&mut self, key: &str, val: isize) -> slog::Result {
        cs!(self, key, val);
        Ok(())
    }

    fn emit_u8(&mut self, key: &str, val: u8) -> slog::Result {
        cs!(self, key, val);
        Ok(())
    }
    fn emit_i8(&mut self, key: &str, val: i8) -> slog::Result {
        cs!(self, key, val);
        Ok(())
    }
    fn emit_u16(&mut self, key: &str, val: u16) -> slog::Result {
        cs!(self, key, val);
        Ok(())
    }
    fn emit_i16(&mut self, key: &str, val: i16) -> slog::Result {
        cs!(self, key, val);
        Ok(())
    }
    fn emit_u32(&mut self, key: &str, val: u32) -> slog::Result {
        cs!(self, key, val);
        Ok(())
    }
    fn emit_i32(&mut self, key: &str, val: i32) -> slog::Result {
        cs!(self, key, val);
        Ok(())
    }
    fn emit_f32(&mut self, key: &str, val: f32) -> slog::Result {
        cs!(self, key, val);
        Ok(())
    }
    fn emit_u64(&mut self, key: &str, val: u64) -> slog::Result {
        cs!(self, key, val);
        Ok(())
    }
    fn emit_i64(&mut self, key: &str, val: i64) -> slog::Result {
        cs!(self, key, val);
        Ok(())
    }
    fn emit_f64(&mut self, key: &str, val: f64) -> slog::Result {
        cs!(self, key, val);
        Ok(())
    }
    fn emit_str(&mut self, key: &str, val: &str) -> slog::Result {
        cs!(self, key, val);
        Ok(())
    }
    fn emit_arguments(&mut self,
                      key: &str,
                      val: &fmt::Arguments)
                      -> slog::Result {
        cs!(self, key, val);
        Ok(())
    }
}
// }}}

// {{{ CountingWriter
// Wrapper for `Write` types that counts total bytes written.
struct CountingWriter<'a> {
    wrapped: &'a mut io::Write,
    count: usize,
}

impl<'a> CountingWriter<'a> {
    fn new(wrapped: &'a mut io::Write) -> CountingWriter {
        CountingWriter {
            wrapped: wrapped,
            count: 0,
        }
    }

    fn count(&self) -> usize {
        self.count
    }
}

impl<'a> io::Write for CountingWriter<'a> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.wrapped
            .write(buf)
            .map(|n| {
                     self.count += n;
                     n
                 })
    }

    fn flush(&mut self) -> io::Result<()> {
        self.wrapped.flush()
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.wrapped
            .write_all(buf)
            .map(|_| {
                     self.count += buf.len();
                     ()
                 })
    }
}
// }}}

// {{{ Timestamp
/// Threadsafe timestamp formatting function type
///
/// To satify `slog-rs` thread and unwind safety requirements, the
/// bounds expressed by this trait need to satisfied for a function
/// to be used in timestamp formatting.
pub trait ThreadSafeTimestampFn
    : Fn(&mut io::Write) ->
    io::Result<()> + Send + Sync + UnwindSafe + RefUnwindSafe + 'static {
}

impl<F> ThreadSafeTimestampFn for F
    where F: Fn(&mut io::Write) -> io::Result<()> + Send + Sync,
          F: UnwindSafe + RefUnwindSafe + 'static,
          F: ?Sized
{
}

const TIMESTAMP_FORMAT: &'static str = "%b %d %H:%M:%S%.3f";

/// Default local timezone timestamp function
///
/// The exact format used, is still subject to change.
pub fn timestamp_local(io: &mut io::Write) -> io::Result<()> {
    write!(io, "{}", chrono::Local::now().format(TIMESTAMP_FORMAT))
}

/// Default UTC timestamp function
///
/// The exact format used, is still subject to change.
pub fn timestamp_utc(io: &mut io::Write) -> io::Result<()> {
    write!(io, "{}", chrono::Utc::now().format(TIMESTAMP_FORMAT))
}
// }}}

// {{{ Plain

/// Plain (no-op) `Decorator` implementation
///
/// This decorator doesn't do any coloring, and doesn't do any synchronization
/// between threads, so is not `Sync`. It is however useful combined with
/// `slog_async::Async` drain, as `slog_async::Async` uses only one thread,
/// and thus requires only `Send` from `Drain`s it wraps.
///
/// ```
/// #[macro_use]
/// extern crate slog;
/// extern crate slog_term;
/// extern crate slog_async;
///
/// use slog::*;
/// use slog_async::Async;
///
/// fn main() {
///
///    let decorator = slog_term::PlainDecorator::new(std::io::stdout());
///    let drain = Async::new(
///            slog_term::FullFormat::new(decorator).build().fuse()
///        )
///        .build()
///        .fuse();
/// }
/// ```

pub struct PlainDecorator<W>(RefCell<W>) where W: io::Write;

impl<W> PlainDecorator<W>
    where W: io::Write
{
    /// Create `PlainDecorator` instance
    pub fn new(io: W) -> Self {
        PlainDecorator(RefCell::new(io))
    }
}

impl<W> Decorator for PlainDecorator<W>
    where W: io::Write + Send + 'static
{
    fn with_record<F>(&self,
                      _record: &Record,
                      _logger_values: &OwnedKVList,
                      f: F)
                      -> io::Result<()>
        where F: FnOnce(&mut RecordDecorator) -> io::Result<()>
    {
        f(&mut PlainRecordDecorator(&self.0))
    }
}

/// Record decorator used by `PlainDecorator`
pub struct PlainRecordDecorator<'a, W: 'a>(&'a RefCell<W>) where W: io::Write;

impl<'a, W> io::Write for PlainRecordDecorator<'a, W>
    where W: io::Write
{
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.borrow_mut().write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.borrow_mut().flush()
    }
}

impl<'a, W> Drop for PlainRecordDecorator<'a, W>
    where W: io::Write
{
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

impl<'a, W> RecordDecorator for PlainRecordDecorator<'a, W>
    where W: io::Write
{
    fn reset(&mut self) -> io::Result<()> {
        Ok(())
    }
}


// }}}

// {{{ PlainSync
/// PlainSync `Decorator` implementation
///
/// This implementation is exactly like `PlainDecorator` but it takes care
/// of synchronizing writes to `io`.
///
/// ```
/// #[macro_use]
/// extern crate slog;
/// extern crate slog_term;
///
/// use slog::*;
///
/// fn main() {
///     let plain = slog_term::PlainSyncDecorator::new(std::io::stdout());
///     let root = Logger::root(
///         slog_term::FullFormat::new(plain).build().fuse(), o!()
///     );
/// }
/// ```

pub struct PlainSyncDecorator<W>(sync::Arc<sync::Mutex<W>>) where W: io::Write;

impl<W> PlainSyncDecorator<W>
    where W: io::Write
{
    /// Create `PlainSyncDecorator` instance
    pub fn new(io: W) -> Self {
        PlainSyncDecorator(sync::Arc::new(sync::Mutex::new(io)))
    }
}

impl<W> Decorator for PlainSyncDecorator<W>
    where W: io::Write + Send + 'static
{
    fn with_record<F>(&self,
                      _record: &Record,
                      _logger_values: &OwnedKVList,
                      f: F)
                      -> io::Result<()>
        where F: FnOnce(&mut RecordDecorator) -> io::Result<()>
    {
        f(&mut PlainSyncRecordDecorator {
                   io: self.0.clone(),
                   buf: vec![],
               })
    }
}

/// `RecordDecorator` used by `PlainSyncDecorator`
pub struct PlainSyncRecordDecorator<W>
    where W: io::Write
{
    io: sync::Arc<sync::Mutex<W>>,
    buf: Vec<u8>,
}

impl<W> io::Write for PlainSyncRecordDecorator<W>
    where W: io::Write
{
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buf.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.buf.is_empty() {
            return Ok(());
        }

        let mut io = try!(self.io
                              .lock()
                              .map_err(|_| {
                                           io::Error::new(io::ErrorKind::Other,
                                                          "mutex locking error")
                                       }));

        try!(io.write_all(&self.buf));
        self.buf.clear();
        io.flush()
    }
}

impl<W> Drop for PlainSyncRecordDecorator<W>
    where W: io::Write
{
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

impl<W> RecordDecorator for PlainSyncRecordDecorator<W>
    where W: io::Write
{
    fn reset(&mut self) -> io::Result<()> {
        Ok(())
    }
}


// }}}

// {{{ TermDecorator

/// Any type of a terminal supported by `term` crate
// TODO: https://github.com/Stebalien/term/issues/70
enum AnyTerminal {
    /// Stdout terminal
    Stdout(Box<term::StdoutTerminal>),
    /// Stderr terminal
    Stderr(Box<term::StderrTerminal>),
}

/// `TermDecorator` builder
pub struct TermDecoratorBuilder {
    term: Option<AnyTerminal>,
    color: Option<bool>,
}

impl TermDecoratorBuilder {
    fn new() -> Self {
        TermDecoratorBuilder {
            term: term::stderr().map(AnyTerminal::Stderr),
            color: None,
        }
    }

    /// Output to `stderr`
    pub fn stderr(mut self) -> Self {
        self.term = term::stderr().map(AnyTerminal::Stderr);
        self
    }

    /// Output to `stdout`
    pub fn stdout(mut self) -> Self {
        self.term = term::stdout().map(AnyTerminal::Stdout);
        self
    }

    /// Force colored output
    pub fn force_color(mut self) -> Self {
        self.color = Some(true);
        self
    }

    /// Force colored output
    pub fn force_plain(mut self) -> Self {
        self.color = Some(false);
        self
    }

    /// Build `TermDecorator`
    ///
    /// Return `None` if requested output was not usable (eg. closed stderr)
    pub fn try_build(self) -> Option<TermDecorator> {
        let use_color = self.should_use_color();
        if let Some(io) = self.term {
            Some(TermDecorator {
                     term: RefCell::new(Some(io)),
                     use_color: use_color,
                 })
        } else {
            None
        }
    }

    /// Build `TermDecorator`
    ///
    /// If requested output was not usable (eg. closed stderr), it will return
    /// `TermDecorator` that discards logging records.
    pub fn build(self) -> TermDecorator {
        let use_color = self.should_use_color();
        TermDecorator {
            term: RefCell::new(self.term),
            use_color: use_color,
        }
    }

    fn should_use_color(&self) -> bool {
        if let Some(color) = self.color {
            return color;
        }
        match self.term {
            Some(AnyTerminal::Stdout(_)) => isatty::stdout_isatty(),
            Some(AnyTerminal::Stderr(_)) => isatty::stderr_isatty(),
            None => false,
        }
    }
}

/// `Decorator` implemented using `term` crate
///
/// This decorator will add nice formatting to the logs it's outputting. It's
/// based on `term` crate.
///
/// It does not deal with serialization so is `!Sync`. Run in a separate thread
/// with `slog_async::Async`.
pub struct TermDecorator {
    term: RefCell<Option<AnyTerminal>>,
    use_color: bool,
}

impl TermDecorator {
    /// Start building `TermDecorator`
    pub fn new() -> TermDecoratorBuilder {
        TermDecoratorBuilder::new()
    }


    /// `Level` color
    ///
    /// Standard level to Unix color conversion used by `TermDecorator`
    pub fn level_to_color(level: slog::Level) -> u16 {
        match level {
            Level::Critical => 5,
            Level::Error => 1,
            Level::Warning => 3,
            Level::Info => 2,
            Level::Debug => 6,
            Level::Trace => 4,
        }
    }
}

impl Decorator for TermDecorator {
    fn with_record<F>(&self,
                      record: &Record,
                      _logger_values: &OwnedKVList,
                      f: F)
                      -> io::Result<()>
        where F: FnOnce(&mut RecordDecorator) -> io::Result<()>
    {
        let mut term = self.term.borrow_mut();
        if let Some(mut term) = term.as_mut() {
            let mut deco = TermRecordDecorator {
                term: term,
                level: record.level(),
                use_color: self.use_color,
            };
            {
                f(&mut deco)
            }
        } else {
            Ok(())
        }
    }
}

/// Record decorator used by `TermDecorator`
pub struct TermRecordDecorator<'a> {
    term: &'a mut AnyTerminal,
    level: slog::Level,
    use_color: bool,
}

impl<'a> io::Write for TermRecordDecorator<'a> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.term {
            &mut AnyTerminal::Stdout(ref mut term) => term.write(buf),
            &mut AnyTerminal::Stderr(ref mut term) => term.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.term {
            &mut AnyTerminal::Stdout(ref mut term) => term.flush(),
            &mut AnyTerminal::Stderr(ref mut term) => term.flush(),
        }
    }
}

impl<'a> Drop for TermRecordDecorator<'a> {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

fn term_error_to_io_error(e: term::Error) -> io::Error {
    match e {
        term::Error::Io(e) => e,
        e => io::Error::new(io::ErrorKind::Other, format!("term error: {}", e)),
    }
}

impl<'a> RecordDecorator for TermRecordDecorator<'a> {
    fn reset(&mut self) -> io::Result<()> {
        if !self.use_color {
            return Ok(());
        }
        match self.term {
                &mut AnyTerminal::Stdout(ref mut term) => term.reset(),
                &mut AnyTerminal::Stderr(ref mut term) => term.reset(),
            }
            .map_err(term_error_to_io_error)
    }

    fn start_level(&mut self) -> io::Result<()> {
        if !self.use_color {
            return Ok(());
        }
        let color = TermDecorator::level_to_color(self.level);
        match self.term {
                &mut AnyTerminal::Stdout(ref mut term) => term.fg(color),
                &mut AnyTerminal::Stderr(ref mut term) => term.fg(color),
            }
            .map_err(term_error_to_io_error)
    }

    fn start_key(&mut self) -> io::Result<()> {
        if !self.use_color {
            return Ok(());
        }
        match self.term {
                &mut AnyTerminal::Stdout(ref mut term) => {
                    if term.supports_attr(term::Attr::Bold) {
                        term.attr(term::Attr::Bold)
                    } else {
                        term.fg(term::color::BRIGHT_WHITE)
                    }
                }
                &mut AnyTerminal::Stderr(ref mut term) => {
                    if term.supports_attr(term::Attr::Bold) {
                        term.attr(term::Attr::Bold)
                    } else {
                        term.fg(term::color::BRIGHT_WHITE)
                    }
                }
            }
            .map_err(term_error_to_io_error)
    }

    fn start_msg(&mut self) -> io::Result<()> {
        // msg is just like key
        self.start_key()
    }
}

// }}}

// {{{ TestStdoutWriter
/// Replacement for `std::io::stdout()` for when output capturing by rust's test harness is
/// required.
pub struct TestStdoutWriter;

impl io::Write for TestStdoutWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        print!("{}", std::str::from_utf8(data)
                              .map_err(|x| io::Error::new(io::ErrorKind::InvalidData, x))?);
        Ok(data.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        io::stdout().flush()
    }
}
// }}}

// {{{ Helpers
/// Create a `CompactFormat` drain with default settings
pub fn term_compact() -> CompactFormat<TermDecorator> {
    let decorator = TermDecorator::new().build();
    CompactFormat::new(decorator).build()
}

/// Create a `FullFormat` drain with default settings
pub fn term_full() -> FullFormat<TermDecorator> {
    let decorator = TermDecorator::new().build();
    FullFormat::new(decorator).build()
}

// }}}


// vim: foldmethod=marker foldmarker={{{,}}}
