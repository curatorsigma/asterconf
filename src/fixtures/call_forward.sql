-- some call forwards to do unit tests with

INSERT INTO call_forward (from_extension, to_extension) VALUES ('702', 'something-external');
INSERT INTO call_forward (from_extension, to_extension) VALUES ('702', '704');
INSERT INTO call_forward (from_extension, to_extension) VALUES ('703', '702');
INSERT INTO call_forward (from_extension, to_extension) VALUES ('704', 'something-external');

INSERT INTO map_call_forward_context (fwd_id, context) VALUES
	(1, 'from_external'),
	(1, 'from_internal'),
	(2, 'from_external'),
	(2, 'from_internal'),
	(3, 'from_extension'),
	(4, 'from_sales');

