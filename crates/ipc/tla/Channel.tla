------------------------------ MODULE Channel ------------------------------
(***************************************************************************)
(* One end of a channel (ipc's core::channel): threads that send, a queue  *)
(* of what the transport hasn't taken, and the one thread that reads the   *)
(* transport (the IO thread), which is the one told that the channel is    *)
(* over. The channel is over when the peer is gone, when the peer sent a   *)
(* bad message, when the queue passed its limit (the peer doesn't read),   *)
(* or when this end closed it.                                             *)
(*                                                                         *)
(* What must hold: the queue never passes its limit; the IO thread is told *)
(* once, with the reason that came first; from then on no message from the *)
(* peer is handed up; and nothing waits to be written on a channel that is *)
(* over. `Mutation` takes one part out; check.py lists the counterexample  *)
(* each must give.                                                         *)
(***************************************************************************)
EXTENDS Naturals

CONSTANTS
    Limit,      \* the most bytes that wait in the queue
    Sizes,      \* the sizes a message can be
    MaxSends,   \* how many sends are tried in all
    Mutation

Mutations == {"none", "no_limit", "reason_overwritten", "told_each_time",
              "deliver_after_over", "queue_kept_when_over", "send_after_over"}

ASSUME Mutation \in Mutations /\ Limit \in Nat /\ MaxSends \in Nat

None == "none"
Reasons == {"peer_gone", "bad_message", "queue_full", "closed"}

VARIABLES
    queued,     \* bytes waiting to be written
    over,       \* why the channel is over, or None
    first,      \* the reason that came first (what `over` must stay)
    told,       \* how many times the IO thread was told
    toldReason, \* what it was told
    sends,      \* sends tried so far
    lateSend,   \* a send was taken after the channel was over
    lateDelivery \* a message was handed up after the IO thread was told

vars == <<queued, over, first, told, toldReason, sends, lateSend, lateDelivery>>

Init ==
    /\ queued = 0
    /\ over = None
    /\ first = None
    /\ told = 0
    /\ toldReason = None
    /\ sends = 0
    /\ lateSend = FALSE
    /\ lateDelivery = FALSE

(* The channel is over for `reason`, unless it already is: the first       *)
(* reason stays. What waited is let go.                                    *)
Finish(reason) ==
    /\ over' = IF over = None \/ Mutation = "reason_overwritten" THEN reason ELSE over
    /\ first' = IF first = None THEN reason ELSE first
    /\ queued' = IF Mutation = "queue_kept_when_over" THEN queued ELSE 0

(* Any thread sends a message the transport has no room for: it waits in   *)
(* the queue, unless that passes the limit.                                *)
Send ==
    /\ sends < MaxSends
    /\ sends' = sends + 1
    /\ \E size \in Sizes :
        IF over # None /\ Mutation # "send_after_over" THEN
            \* Refused: the sender is told the channel is over.
            UNCHANGED <<queued, over, first, lateSend>>
        ELSE IF queued + size > Limit /\ Mutation # "no_limit" THEN
            /\ Finish("queue_full")
            /\ UNCHANGED lateSend
        ELSE
            /\ queued' = queued + size
            /\ lateSend' = (lateSend \/ over # None)
            /\ UNCHANGED <<over, first>>
    /\ UNCHANGED <<told, toldReason, lateDelivery>>

(* The transport takes some of what waits.                                 *)
Flush ==
    /\ over = None /\ queued > 0
    /\ \E n \in 1..queued : queued' = queued - n
    /\ UNCHANGED <<over, first, told, toldReason, sends, lateSend, lateDelivery>>

(* The platform says the peer is gone, the peer's bytes hold a bad         *)
(* message, or a thread of this end closes the channel.                    *)
Ends ==
    /\ \E reason \in {"peer_gone", "bad_message", "closed"} : Finish(reason)
    /\ UNCHANGED <<told, toldReason, sends, lateSend, lateDelivery>>

(* The IO thread comes by (bytes to feed, a wake, the platform's word):    *)
(* if the channel is over and it hasn't been told, it is told now.         *)
Tell ==
    /\ over # None
    /\ told = 0 \/ Mutation = "told_each_time"
    /\ told < 3
    /\ told' = told + 1
    /\ toldReason' = over
    /\ UNCHANGED <<queued, over, first, sends, lateSend, lateDelivery>>

(* The IO thread feeds bytes that hold a whole good message: handed up,    *)
(* unless the channel is over.                                             *)
Deliver ==
    /\ over = None \/ Mutation = "deliver_after_over"
    /\ lateDelivery' = (lateDelivery \/ told > 0)
    /\ UNCHANGED <<queued, over, first, told, toldReason, sends, lateSend>>

Next == Send \/ Flush \/ Ends \/ Tell \/ Deliver

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------

TypeOK ==
    /\ queued \in Nat
    /\ over \in Reasons \cup {None}
    /\ first \in Reasons \cup {None}
    /\ told \in 0..3
    /\ toldReason \in Reasons \cup {None}
    /\ sends \in 0..MaxSends
    /\ lateSend \in BOOLEAN /\ lateDelivery \in BOOLEAN

(* The queue never holds more than its limit.                              *)
QueueBound == queued <= Limit

(* The IO thread is told the channel is over once.                         *)
ToldOnce == told <= 1

(* What it is told is the reason that came first.                          *)
ToldFirstReason == (told > 0) => toldReason = first

(* The reason doesn't change once there is one.                            *)
ReasonStays == over = first

(* Once it is told, no message from the peer is handed up.                 *)
NoLateDelivery == ~lateDelivery

(* Nothing waits on a channel that is over, and nothing is taken to wait.  *)
NothingWaitsWhenOver == (over # None) => queued = 0
NoLateSend == ~lateSend

=============================================================================
