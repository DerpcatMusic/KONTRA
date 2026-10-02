# Derpcat Flight Recorder

A bounded structured diagnostic recorder for audio applications and plug-ins.

Normal threads emit typed events through a non-blocking sink. Real-time audio
threads use a fixed-size `Copy` event and a preallocated SPSC ring. A background
writer persists recent events in checksummed fixed-size journal slots so a torn
crash-time write does not destroy the rest of the session history.

The crate records evidence only. Product code owns consent, redaction, incident
presentation, upload policy, and native crash-artifact collection.
