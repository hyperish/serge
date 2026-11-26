"""Executable examples of the WebView v1 event protocol for one host slot."""

from dataclasses import dataclass
from itertools import product
from unittest import TestCase, main


@dataclass(frozen=True)
class Event:
    seq: int
    id: str
    payload: str


FIRST = Event(1, "a", "save-first")
SECOND = Event(2, "b", "save-second")


class Slot:
    def __init__(self):
        self.session = "session-a"
        self.generation = 1
        self.last = None
        self.completed = False
        self.ready = False
        self.effects = []

    def reload(self):
        self.generation += 1
        self.ready = False

    def accept(self, session, generation, event):
        if (session, generation) != (self.session, self.generation) or not self.ready:
            return "stale"
        if self.last is not None:
            if event == self.last:
                return "done" if self.completed else "processing"
            if event.id == self.last.id or event.seq == self.last.seq:
                return "resync"
        expected = self.last.seq + 1 if self.last else 1
        if event.seq != expected:
            return "resync"
        if self.last is not None and not self.completed:
            return "busy"
        self.last = event
        self.completed = False
        self.effects.append(event.id)
        return "processing"


class Page:
    def __init__(self, slot):
        # One event is recorded before its first delivery. The record survives a reload.
        self.slot = slot
        self.pending = FIRST
        self.stale_frame = None
        self.slot.ready = True

    def step(self, action):
        slot = self.slot
        if action == "send" and self.pending is not None:
            slot.accept(slot.session, slot.generation, self.pending)
        elif action == "finish" and slot.last is not None:
            slot.completed = True
        elif action == "receive_result" and slot.completed and slot.last == self.pending:
            self.pending = None
        elif action == "reload":
            if self.pending is not None:
                self.stale_frame = (slot.session, slot.generation, self.pending)
            slot.reload()
            slot.ready = True  # Hello/Ready for the new document
        elif action == "late_delivery" and self.stale_frame is not None:
            return slot.accept(*self.stale_frame)
        elif action == "submit_next" and self.pending is None and slot.last == FIRST:
            self.pending = SECOND


class EventOrdering(TestCase):
    def test_recovery_after_five_delivery_steps(self):
        actions = ("send", "finish", "receive_result", "reload", "late_delivery", "submit_next")
        recovery = ("send", "finish", "receive_result", "submit_next", "send", "finish", "receive_result")
        for prefix in product(actions, repeat=5):
            slot = Slot()
            page = Page(slot)
            for action in prefix + recovery:
                reply = page.step(action)
                if reply is not None:
                    self.assertEqual(reply, "stale", prefix)
            self.assertEqual(slot.effects, [FIRST.id, SECOND.id], prefix)
            self.assertIsNone(page.pending, prefix)

    def test_gaps_duplicates_and_document_identity(self):
        slot = Slot()
        slot.ready = True

        def deliver(event):
            return slot.accept(slot.session, slot.generation, event)

        self.assertEqual(slot.accept("another-session", 1, FIRST), "stale")
        self.assertEqual(deliver(SECOND), "resync")
        self.assertEqual(deliver(FIRST), "processing")
        self.assertEqual(deliver(FIRST), "processing")
        self.assertEqual(deliver(Event(1, FIRST.id, "different")), "resync")
        self.assertEqual(deliver(SECOND), "busy")

        slot.reload()
        self.assertEqual(slot.accept(slot.session, 1, FIRST), "stale")
        self.assertEqual(deliver(FIRST), "stale")
        slot.ready = True
        slot.completed = True
        self.assertEqual(deliver(FIRST), "done")
        self.assertEqual(deliver(SECOND), "processing")
        self.assertEqual(slot.effects, [FIRST.id, SECOND.id])


if __name__ == "__main__":
    main()
