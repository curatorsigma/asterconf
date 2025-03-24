CREATE TABLE map_call_forward_timeframe
	( fwd_id INTEGER REFERENCES call_forward(fwd_id) NOT NULL
	, once_id INTEGER REFERENCES timeframe_once(once_id) ON DELETE CASCADE
	, daily_id INTEGER REFERENCES timeframe_daily(daily_id) ON DELETE CASCADE
	, weekly_id INTEGER REFERENCES timeframe_weekly(weekly_id) ON DELETE CASCADE
	, monthly_id INTEGER REFERENCES timeframe_monthly(monthly_id) ON DELETE CASCADE
	, constraint timeframe_type_well_def CHECK (num_nonnulls(once_id, daily_id, weekly_id, monthly_id) = 1)
);

