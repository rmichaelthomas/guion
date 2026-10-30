# JSONL streaming summary with text_chars date extraction

This is a controlled ablation of jsonl-streaming-summary/main.gu. It changes only the UTC-date extraction expression from text_slice to text_chars plus indexed character concatenation. Parsing, validation, aggregation representation, MapRecord updates, output construction, and input are identical. The paired benchmark compares this lane with the text_slice source to isolate the date-extraction contribution.
