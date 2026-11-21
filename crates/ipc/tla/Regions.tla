------------------------------ MODULE Regions ------------------------------
(***************************************************************************)
(* The regions one end writes large data into and its peers read (ipc's    *)
(* core::region): the writer (a Browser) makes regions, each for one peer, *)
(* and tells that peer with a message before any message that names the    *)
(* region. It writes a piece of data into a free region of that peer's,    *)
(* sends a message naming the region, and writes the region again only     *)
(* once the peer said it had read it. A peer reads what a message names    *)
(* when it gets to that message, which can be later than when the message  *)
(* arrived, and then says so.                                              *)
(*                                                                         *)
(* When a peer goes, its regions are let go: they are never another        *)
(* peer's, for what the first wrote or was sent is still in them.          *)
(*                                                                         *)
(* A hostile peer's messages (a region not its own, one given back twice)  *)
(* are the fuzzer's and the unit tests': here the peers are ours, and the  *)
(* model checks that ours never trip those checks and never read what they *)
(* weren't sent. `Mutation` takes one part out; check.py lists the         *)
(* counterexample each must give.                                          *)
(***************************************************************************)
EXTENDS Naturals, Sequences, FiniteSets

CONSTANTS
    Peers,      \* the peers (renderers)
    Regions,    \* the regions that can be made
    MaxData,    \* how many pieces of data are sent in all
    Mutation

Mutations == {"none", "reuse_without_release", "release_on_arrival", "use_before_grant",
              "release_twice", "regrant_after_peer_gone", "grant_to_two"}

ASSUME Mutation \in Mutations /\ MaxData \in Nat

None == "none"

VARIABLES
    owner,      \* region -> the peer it was made for, or None
    everOwned,  \* region -> every peer that ever had it
    out,        \* region -> something written in it hasn't been read
    content,    \* region -> the piece of data in it (0: none yet)
    nextData,   \* the next piece's number
    toPeer,     \* peer -> messages on their way to it, in order
    toWriter,   \* peer -> messages on their way back, in order
    table,      \* peer -> the regions it was told of
    pending,    \* peer -> messages arrived and not yet got to, in order
    up,         \* peer -> its channel is up
    wrongRead,  \* a peer read a piece other than the one it was sent
    peerBad,    \* a peer was named a region it hadn't been told of
    writerBad   \* the writer took a peer's message for a bad one

vars == <<owner, everOwned, out, content, nextData, toPeer, toWriter, table,
          pending, up, wrongRead, peerBad, writerBad>>

Init ==
    /\ owner = [r \in Regions |-> None]
    /\ everOwned = [r \in Regions |-> {}]
    /\ out = [r \in Regions |-> FALSE]
    /\ content = [r \in Regions |-> 0]
    /\ nextData = 1
    /\ toPeer = [p \in Peers |-> <<>>]
    /\ toWriter = [p \in Peers |-> <<>>]
    /\ table = [p \in Peers |-> {}]
    /\ pending = [p \in Peers |-> <<>>]
    /\ up = [p \in Peers |-> TRUE]
    /\ wrongRead = FALSE
    /\ peerBad = FALSE
    /\ writerBad = FALSE

(* The writer makes a region for a peer and tells it.                      *)
Grant(p, r) ==
    /\ up[p]
    /\ \/ owner[r] = None /\ everOwned[r] = {}
       \/ owner[r] = None /\ Mutation = "regrant_after_peer_gone"
       \/ owner[r] # None /\ owner[r] # p /\ Mutation = "grant_to_two"
    /\ owner' = [owner EXCEPT ![r] = p]
    /\ everOwned' = [everOwned EXCEPT ![r] = @ \cup {p}]
    /\ out' = [out EXCEPT ![r] = FALSE]
    /\ toPeer' = [toPeer EXCEPT ![p] = Append(@, [type |-> "grant", r |-> r, d |-> 0])]
    /\ UNCHANGED <<content, nextData, toWriter, table, pending, up, wrongRead,
                   peerBad, writerBad>>

(* The writer puts a piece of data in a free region of the peer's and      *)
(* sends the message that names it.                                        *)
Put(p, r) ==
    /\ up[p] /\ nextData <= MaxData
    /\ \/ owner[r] = p
       \/ Mutation = "use_before_grant" /\ owner[r] = None /\ everOwned[r] = {}
    /\ ~out[r] \/ Mutation = "reuse_without_release"
    /\ content' = [content EXCEPT ![r] = nextData]
    /\ out' = [out EXCEPT ![r] = TRUE]
    /\ nextData' = nextData + 1
    /\ IF owner[r] = p THEN
           /\ toPeer' = [toPeer EXCEPT ![p] = Append(@, [type |-> "use", r |-> r, d |-> nextData])]
           /\ UNCHANGED <<owner, everOwned>>
       ELSE
           \* The mutation: the message that names the region goes first.
           /\ toPeer' = [toPeer EXCEPT ![p] =
                  Append(Append(@, [type |-> "use", r |-> r, d |-> nextData]),
                         [type |-> "grant", r |-> r, d |-> 0])]
           /\ owner' = [owner EXCEPT ![r] = p]
           /\ everOwned' = [everOwned EXCEPT ![r] = @ \cup {p}]
    /\ UNCHANGED <<toWriter, table, pending, up, wrongRead, peerBad, writerBad>>

(* A peer's channel hands it the next message: a region it is told of, or  *)
(* a message it will get to.                                               *)
PeerReceive(p) ==
    /\ up[p] /\ toPeer[p] # <<>>
    /\ LET m == Head(toPeer[p]) IN
        /\ toPeer' = [toPeer EXCEPT ![p] = Tail(@)]
        /\ IF m.type = "grant" THEN
               /\ table' = [table EXCEPT ![p] = @ \cup {m.r}]
               /\ UNCHANGED <<pending, peerBad, toWriter>>
           ELSE
               /\ pending' = [pending EXCEPT ![p] = Append(@, m)]
               /\ peerBad' = (peerBad \/ m.r \notin table[p])
               /\ toWriter' = IF Mutation = "release_on_arrival"
                              THEN [toWriter EXCEPT ![p] = Append(@, m.r)]
                              ELSE toWriter
               /\ UNCHANGED table
    /\ UNCHANGED <<owner, everOwned, out, content, nextData, up, wrongRead, writerBad>>

(* A peer gets to a message: it reads the region the message names, then   *)
(* says it has.                                                            *)
PeerRead(p) ==
    /\ up[p] /\ pending[p] # <<>>
    /\ LET m == Head(pending[p]) IN
        /\ pending' = [pending EXCEPT ![p] = Tail(@)]
        /\ wrongRead' = (wrongRead \/ content[m.r] # m.d)
        /\ toWriter' =
               IF Mutation = "release_on_arrival" THEN toWriter
               ELSE IF Mutation = "release_twice"
                    THEN [toWriter EXCEPT ![p] = Append(Append(@, m.r), m.r)]
               ELSE [toWriter EXCEPT ![p] = Append(@, m.r)]
    /\ UNCHANGED <<owner, everOwned, out, content, nextData, toPeer, table, up,
                   peerBad, writerBad>>

(* The writer hears a region was read: free again. One that isn't that     *)
(* peer's, or that nothing is out in, is a bad message.                    *)
WriterReceive(p) ==
    /\ up[p] /\ toWriter[p] # <<>>
    /\ LET r == Head(toWriter[p]) IN
        /\ toWriter' = [toWriter EXCEPT ![p] = Tail(@)]
        /\ IF owner[r] # p \/ ~out[r] THEN
               /\ writerBad' = TRUE
               /\ UNCHANGED out
           ELSE
               /\ out' = [out EXCEPT ![r] = FALSE]
               /\ UNCHANGED writerBad
    /\ UNCHANGED <<owner, everOwned, content, nextData, toPeer, table, pending, up,
                   wrongRead, peerBad>>

(* A peer goes: its regions are let go, and are nobody's from then on.     *)
PeerGone(p) ==
    /\ up[p]
    /\ up' = [up EXCEPT ![p] = FALSE]
    /\ owner' = [r \in Regions |-> IF owner[r] = p THEN None ELSE owner[r]]
    /\ out' = [r \in Regions |-> IF owner[r] = p THEN FALSE ELSE out[r]]
    /\ toPeer' = [toPeer EXCEPT ![p] = <<>>]
    /\ toWriter' = [toWriter EXCEPT ![p] = <<>>]
    /\ pending' = [pending EXCEPT ![p] = <<>>]
    /\ table' = [table EXCEPT ![p] = {}]
    /\ UNCHANGED <<everOwned, content, nextData, wrongRead, peerBad, writerBad>>

Next ==
    \/ \E p \in Peers, r \in Regions : Grant(p, r) \/ Put(p, r)
    \/ \E p \in Peers : PeerReceive(p) \/ PeerRead(p) \/ WriterReceive(p) \/ PeerGone(p)

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------

TypeOK ==
    /\ owner \in [Regions -> Peers \cup {None}]
    /\ everOwned \in [Regions -> SUBSET Peers]
    /\ out \in [Regions -> BOOLEAN]
    /\ content \in [Regions -> 0..MaxData]
    /\ nextData \in 1..(MaxData + 1)
    /\ table \in [Peers -> SUBSET Regions]
    /\ up \in [Peers -> BOOLEAN]
    /\ wrongRead \in BOOLEAN /\ peerBad \in BOOLEAN /\ writerBad \in BOOLEAN

(* A peer reads the piece of data the message it got to was sent with:     *)
(* the region wasn't written again under it.                               *)
ReadsWhatWasSent == ~wrongRead

(* A message never names a region its peer wasn't told of first.           *)
ToldBeforeNamed == ~peerBad

(* Peers of ours never give back what the writer takes for a bad message.  *)
NoFalseBad == ~writerBad

(* A region is only ever one peer's: not another's while the first has it, *)
(* nor after the first is gone.                                            *)
OnePeerEver == \A r \in Regions : Cardinality(everOwned[r]) <= 1

(* A peer's table holds only regions that are its own.                     *)
TableIsOwn == \A p \in Peers : \A r \in table[p] : up[p] => everOwned[r] = {p}

=============================================================================
