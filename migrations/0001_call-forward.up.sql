-- some comment

CREATE TABLE call_forward (
	fwd_id serial PRIMARY KEY,
	from_extension TEXT NOT NULL,
	to_extension TEXT NOT NULL
);

