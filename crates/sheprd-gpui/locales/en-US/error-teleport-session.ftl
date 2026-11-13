error-teleport-session-configuration = { $agent } uses a custom session configuration path; Teleport needs an explicit destination mapping.
error-teleport-session-resume-unconfirmed = { $agent } did not acknowledge the transferred session on { $host }; the source keeps running.
error-teleport-session-destination-tabs-open = An old destination tab stayed open: { $error }
error-teleport-session-arrival =
    { $reason ->
        [full] Too many pending Teleport arrivals; reopen the app to recover them first.
        [io] Could not persist the Teleport arrival.
        [json] Invalid Teleport arrival record.
        [size] Teleport arrival record exceeds its size limit.
        [unavailable] The app has no state directory for a durable Teleport arrival.
       *[version] Unsupported Teleport arrival record version.
    }
