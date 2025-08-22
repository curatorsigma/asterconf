-- Remove ON DELETE CASCADE to map_call_forward_timeframe FK to call_forward
ALTER TABLE map_call_forward_timeframe DROP CONSTRAINT map_call_forward_timeframe_fwd_id_fkey, ADD  CONSTRAINT map_call_forward_timeframe_fwd_id_fkey FOREIGN KEY (fwd_id) REFERENCES call_forward(fwd_id);

