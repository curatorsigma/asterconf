-- Create the table that 1:n maps a call forward to the contexts from which it is active
CREATE TABLE map_call_forward_context (
	fwd_id integer NOT NULL REFERENCES call_forward(fwd_id) ON UPDATE CASCADE ON DELETE CASCADE,
	context TEXT NOT NULL
);

