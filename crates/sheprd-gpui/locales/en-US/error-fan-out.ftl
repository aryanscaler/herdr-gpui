# Fan-out failures. $step is the part that failed; translate each variant,
# not the keys.

# $error says why, in herdr's words or the system's.
error-fan-out-script =
    { $step ->
        [detect] finding installed agents failed: { $error }
        [start-agent] starting the agent failed: { $error }
       *[compare] reading the changes failed: { $error }
    }
error-fan-out-no-base = The base commit could not be read
# $host is a device's name.
error-fan-out-host-unavailable = { $host } could not be set up
# $agent is the agent's name.
error-fan-out-agent-missing = { $agent } is not installed on { $host }
error-fan-out-cancelled = Fan-out cancelled
error-fan-out-journal-read = The saved fan-outs could not be read
error-fan-out-journal-json = The saved fan-outs are not valid JSON
error-fan-out-journal-write = The fan-out's progress could not be saved
error-fan-out-journal-full = Too many fan-outs are saved; this one will not come back after a quit. Forget one to make room.
error-fan-out-journal-field-too-long = A name or path in this fan-out is too long to save; it will not come back after a quit.
error-fan-out-agent-uncertain = The agent may already be running, but its terminal was not saved. Keep the checkout and inspect it before starting another agent.
