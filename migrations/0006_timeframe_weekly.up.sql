CREATE TYPE DAYOFWEEK AS ENUM('Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday');

CREATE TABLE timeframe_weekly
( weekly_id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY
, start_dow DAYOFWEEK NOT NULL
, start_time TIME NOT NULL
, end_dow DAYOFWEEK NOT NULL
, end_time TIME NOT NULL
);

