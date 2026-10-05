# Load forecast service

This service predicts electricity consumption per district for the next 48 hours. It reads meter data from the data lake every hour, trains a gradient boosting model per district once a night, and writes the forecasts to the database that the dashboard reads. Run it locally with `make run`; the tests need a PostgreSQL container.
