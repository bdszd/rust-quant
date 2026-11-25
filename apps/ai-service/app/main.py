from fastapi import FastAPI

app = FastAPI(
    title="Quant AI Service",
    description="AI-powered quantitative research assistant",
    version="0.1.0",
)


@app.get("/health")
async def health():
    return {
        "status": "ok",
        "service": "quant-ai-service",
    }