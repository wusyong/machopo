-------------------------------- MODULE Pipe --------------------------------
(***************************************************************************)
(* One data pipe across a channel (ipc's core::pipe): a writer's end that  *)
(* sends the pipe's bytes in pieces, a reader's end that holds them until  *)
(* they are read, and the two streams of messages between them, each in    *)
(* order. Either end may be cut off at any time.                           *)
(*                                                                         *)
(* The writer sends no more than the pipe's capacity past what the reader  *)
(* said it read. The reader says so once what it hasn't said reaches a     *)
(* threshold (half the capacity). The writer ends the pipe; the reader may *)
(* cancel it. A message for a pipe that is over is let go: a report can    *)
(* cross the pipe's end, bytes can cross a cancel.                         *)
(*                                                                         *)
(* Bytes are counted, not told apart: the channel keeps their order        *)
(* (core::frame's sequence numbers).                                       *)
(*                                                                         *)
(* `Hostile` lets the reader's end send any report and cancel at any time, *)
(* as a renderer that is not ours would. `Mutation` takes one part out;    *)
(* check.py lists the counterexample each must give.                       *)
(***************************************************************************)
EXTENDS Integers, Sequences

CONSTANTS
    Capacity,   \* the most the reader holds unread
    Total,      \* the bytes the writer has to send
    MaxChunk,   \* the most in one message
    Hostile,    \* the reader's end sends whatever it likes
    Mutation    \* "none", or the part of the protocol taken out

Mutations == {"none", "no_window", "threshold_over_capacity", "no_report_reset",
              "no_overread_check", "stale_report_is_bad", "stale_data_is_bad",
              "cancel_keeps_writing"}

ASSUME Capacity \in Nat \ {0} /\ Total \in Nat /\ MaxChunk \in Nat \ {0}
ASSUME Hostile \in BOOLEAN /\ Mutation \in Mutations

VARIABLES
    alive,       \* the channel is up
    toSend,      \* the writer: bytes not yet sent
    unread,      \* the writer: sent and not reported read (an integer: a
                 \* missing check lets it go under zero)
    writer,      \* "open", "ended" or "cancelled"
    toReader,    \* messages on their way to the reader, in order
    toWriter,    \* messages on their way to the writer, in order
    held,        \* the reader: received and not read
    unreported,  \* the reader: read and not yet reported
    reader,      \* "open", "ended" (the end came) or "cancelled"
    delivered,   \* bytes the reader's user read
    writerBad,   \* the writer took a message for a bad one
    readerBad    \* the reader did

vars == <<alive, toSend, unread, writer, toReader, toWriter, held, unreported,
          reader, delivered, writerBad, readerBad>>

Threshold == IF Mutation = "threshold_over_capacity" THEN Capacity + 1
             ELSE IF Capacity >= 2 THEN Capacity \div 2 ELSE 1

Min(a, b) == IF a < b THEN a ELSE b

Room == IF Mutation = "no_window" THEN Capacity ELSE Capacity - unread

Init ==
    /\ alive = TRUE
    /\ toSend = Total
    /\ unread = 0
    /\ writer = "open"
    /\ toReader = <<>>
    /\ toWriter = <<>>
    /\ held = 0
    /\ unreported = 0
    /\ reader = "open"
    /\ delivered = 0
    /\ writerBad = FALSE
    /\ readerBad = FALSE

(* The writer sends a piece: as much as there is room for, at most.        *)
Send ==
    /\ alive /\ writer = "open"
    /\ \E n \in 1..Min(MaxChunk, Min(toSend, Room)) :
        /\ toSend' = toSend - n
        /\ unread' = unread + n
        /\ toReader' = Append(toReader, [type |-> "data", n |-> n])
    /\ UNCHANGED <<alive, writer, toWriter, held, unreported, reader, delivered,
                   writerBad, readerBad>>

(* The writer has sent everything: the end.                                *)
End ==
    /\ alive /\ writer = "open" /\ toSend = 0
    /\ writer' = "ended"
    /\ toReader' = Append(toReader, [type |-> "end", n |-> 0])
    /\ UNCHANGED <<alive, toSend, unread, toWriter, held, unreported, reader,
                   delivered, writerBad, readerBad>>

(* The writer takes the next message from the reader.                      *)
WriterReceive ==
    /\ alive /\ toWriter # <<>>
    /\ LET m == Head(toWriter) IN
        /\ toWriter' = Tail(toWriter)
        /\ IF m.type = "read" THEN
               IF writer # "open" THEN
                   \* A report that crossed the pipe's end: let go.
                   /\ writerBad' = (writerBad \/ Mutation = "stale_report_is_bad")
                   /\ UNCHANGED <<unread, writer>>
               ELSE IF m.n > unread /\ Mutation # "no_overread_check" THEN
                   /\ writerBad' = TRUE
                   /\ UNCHANGED <<unread, writer>>
               ELSE
                   /\ unread' = unread - m.n
                   /\ UNCHANGED <<writer, writerBad>>
           ELSE
               \* A cancel: the writer stops.
               /\ writer' = IF writer = "open" /\ Mutation # "cancel_keeps_writing"
                            THEN "cancelled" ELSE writer
               /\ UNCHANGED <<unread, writerBad>>
    /\ alive' = ~writerBad'
    /\ UNCHANGED <<toSend, toReader, held, unreported, reader, delivered, readerBad>>

(* The reader's end takes the next message from the writer.                *)
ReaderReceive ==
    /\ alive /\ toReader # <<>>
    /\ LET m == Head(toReader) IN
        /\ toReader' = Tail(toReader)
        /\ IF m.type = "data" THEN
               IF reader = "cancelled" THEN
                   \* Bytes that crossed the cancel: let go.
                   /\ readerBad' = (readerBad \/ Mutation = "stale_data_is_bad")
                   /\ UNCHANGED <<held, reader>>
               ELSE IF held + m.n > Capacity THEN
                   \* More than the pipe holds: the writer's mistake.
                   /\ readerBad' = TRUE
                   /\ UNCHANGED <<held, reader>>
               ELSE
                   /\ held' = held + m.n
                   /\ UNCHANGED <<reader, readerBad>>
           ELSE
               /\ reader' = IF reader = "open" THEN "ended" ELSE reader
               /\ UNCHANGED <<held, readerBad>>
    /\ alive' = ~readerBad'
    /\ UNCHANGED <<toSend, unread, writer, toWriter, unreported, delivered, writerBad>>

(* The reader's user reads some of what is held; past the threshold, the   *)
(* reader says how much it read since it last said.                        *)
Read ==
    /\ alive /\ reader # "cancelled" /\ ~Hostile
    /\ \E n \in 1..held :
        /\ held' = held - n
        /\ delivered' = delivered + n
        /\ IF unreported + n >= Threshold THEN
               /\ toWriter' = Append(toWriter, [type |-> "read", n |-> unreported + n])
               /\ unreported' = IF Mutation = "no_report_reset" THEN unreported + n ELSE 0
           ELSE
               /\ unreported' = unreported + n
               /\ UNCHANGED toWriter
    /\ UNCHANGED <<alive, toSend, unread, writer, toReader, reader, writerBad, readerBad>>

(* The reader's user wants no more: what is held is let go.                *)
Cancel ==
    /\ alive /\ reader = "open" /\ ~Hostile
    /\ reader' = "cancelled"
    /\ held' = 0
    /\ toWriter' = Append(toWriter, [type |-> "cancel", n |-> 0])
    /\ UNCHANGED <<alive, toSend, unread, writer, toReader, unreported, delivered,
                   writerBad, readerBad>>

(* A reader's end that is not ours: any report, a cancel, at any time (a   *)
(* few at once at most, to keep the model finite).                         *)
HostileSend ==
    /\ alive /\ Hostile /\ Len(toWriter) < 2
    /\ \/ \E n \in 0..(Capacity + 1) :
              toWriter' = Append(toWriter, [type |-> "read", n |-> n])
       \/ toWriter' = Append(toWriter, [type |-> "cancel", n |-> 0])
    /\ UNCHANGED <<alive, toSend, unread, writer, toReader, held, unreported, reader,
                   delivered, writerBad, readerBad>>

(* Either process goes, or the channel is closed.                          *)
Cut ==
    /\ alive
    /\ alive' = FALSE
    /\ UNCHANGED <<toSend, unread, writer, toReader, toWriter, held, unreported,
                   reader, delivered, writerBad, readerBad>>

Next == Send \/ End \/ WriterReceive \/ ReaderReceive \/ Read \/ Cancel \/ HostileSend \/ Cut

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------

Message == [type : {"data", "end", "read", "cancel"}, n : 0..(Capacity + Total + 1)]

TypeOK ==
    /\ alive \in BOOLEAN
    /\ toSend \in 0..Total
    /\ unread \in Int
    /\ writer \in {"open", "ended", "cancelled"}
    /\ toReader \in Seq(Message)
    /\ toWriter \in Seq(Message)
    /\ held \in Nat
    /\ unreported \in Nat
    /\ reader \in {"open", "ended", "cancelled"}
    /\ delivered \in 0..Total
    /\ writerBad \in BOOLEAN
    /\ readerBad \in BOOLEAN

(* The writer's count of what is sent and not reported read never leaves   *)
(* the pipe's capacity: holds whatever the reader's end sends.             *)
WindowBound == 0 <= unread /\ unread <= Capacity

(* Ends of ours never take each other's messages for bad ones.             *)
NoFalseBad == ~Hostile => (~writerBad /\ ~readerBad)

(* The bytes on their way to the reader.                                   *)
RECURSIVE Bytes(_)
Bytes(s) == IF s = <<>> THEN 0
            ELSE (IF Head(s).type = "data" THEN Head(s).n ELSE 0) + Bytes(Tail(s))

(* What the writer counts as unread is what the reader hasn't reported:    *)
(* on its way, held, read and not yet said, or said and on its way back.   *)
RECURSIVE Reports(_)
Reports(s) == IF s = <<>> THEN 0
              ELSE (IF Head(s).type = "read" THEN Head(s).n ELSE 0) + Reports(Tail(s))

Accounted ==
    (~Hostile /\ alive /\ writer = "open" /\ reader # "cancelled")
        => unread = Bytes(toReader) + held + unreported + Reports(toWriter)

(* No byte is lost or made up: while nothing is cancelled, what the user   *)
(* read, what is held, what is on its way and what is yet to send are all  *)
(* of them.                                                                *)
Conserved ==
    (~Hostile /\ alive /\ writer # "cancelled" /\ reader # "cancelled")
        => delivered + held + Bytes(toReader) + toSend = Total

(* A writer with bytes to send is never out of room with nothing on its    *)
(* way in either direction and nothing held: it would wait for ever.       *)
NotStuck ==
    ~(/\ ~Hostile /\ alive /\ writer = "open" /\ reader = "open"
      /\ toSend > 0 /\ Capacity - unread = 0
      /\ toReader = <<>> /\ toWriter = <<>> /\ held = 0)

(* Once a cancel has reached the writer, the writer has stopped.           *)
CancelHeard ==
    (~Hostile /\ alive /\ reader = "cancelled" /\ toWriter = <<>>) => writer # "open"

=============================================================================
